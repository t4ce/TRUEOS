#[path = "../src/localcoder.rs"]
mod localcoder;

#[cfg(unix)]
mod unix_tests {
    use super::localcoder::{
        chat, status, LocalCoderBinSource, LocalCoderChatRequest, LocalCoderCommandStrategy,
        LOCALCODER_ARGS_ENV, LOCALCODER_BIN_ENV,
    };
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct ScopedEnv {
        _guard: MutexGuard<'static, ()>,
        old_bin: Option<OsString>,
        old_args: Option<OsString>,
        old_path: Option<OsString>,
        old_cwd: PathBuf,
    }

    impl ScopedEnv {
        fn new(cwd: &Path) -> Self {
            let guard = ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let old = Self {
                _guard: guard,
                old_bin: env::var_os(LOCALCODER_BIN_ENV),
                old_args: env::var_os(LOCALCODER_ARGS_ENV),
                old_path: env::var_os("PATH"),
                old_cwd: env::current_dir().unwrap(),
            };

            remove_env(LOCALCODER_BIN_ENV);
            remove_env(LOCALCODER_ARGS_ENV);
            set_env("PATH", "");
            env::set_current_dir(cwd).unwrap();

            old
        }

        fn set<K, V>(&self, key: K, value: V)
        where
            K: AsRef<OsStr>,
            V: AsRef<OsStr>,
        {
            set_env(key, value);
        }

        fn remove<K>(&self, key: K)
        where
            K: AsRef<OsStr>,
        {
            remove_env(key);
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            restore_env(LOCALCODER_BIN_ENV, self.old_bin.as_ref());
            restore_env(LOCALCODER_ARGS_ENV, self.old_args.as_ref());
            restore_env("PATH", self.old_path.as_ref());
            let _ = env::set_current_dir(&self.old_cwd);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn status_uses_localcoder_bin_first() {
        let temp = temp_dir("status-bin-first");
        let fake = temp.join("fake-localcoder");
        write_executable(&fake, "#!/bin/sh\nprintf 'unused\\n'\n");
        let env = ScopedEnv::new(&temp);
        env.set(LOCALCODER_BIN_ENV, fake.as_os_str());

        let response = status().await;

        assert!(response.available);
        assert_eq!(response.source, Some(LocalCoderBinSource::EnvVar));
        assert_eq!(
            response.bin_path.as_deref(),
            Some(fs::canonicalize(&fake).unwrap().to_string_lossy().as_ref())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chat_default_sends_prompt_on_stdin() {
        let temp = temp_dir("chat-stdin");
        let fake = temp.join("fake-localcoder");
        write_executable(&fake, "#!/bin/sh\nIFS= read -r input\nprintf 'reply:%s\\n' \"$input\"\n");
        let env = ScopedEnv::new(&temp);
        env.set(LOCALCODER_BIN_ENV, fake.as_os_str());
        env.remove(LOCALCODER_ARGS_ENV);

        let response = chat(LocalCoderChatRequest {
            prompt: "hello local coder".to_string(),
            timeout_ms: Some(5_000),
        })
        .await
        .unwrap();

        assert_eq!(response.text, "reply:hello local coder");
        assert_eq!(response.strategy, LocalCoderCommandStrategy::DefaultStdin);
        assert_eq!(response.exit_code, Some(0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chat_env_args_substitutes_prompt_as_one_arg() {
        let temp = temp_dir("chat-env-args");
        let fake = temp.join("fake-localcoder");
        write_executable(
            &fake,
            "#!/bin/sh\nprintf 'argc=%s\\n' \"$#\"\ni=1\nfor arg do\n  printf 'arg%s=%s\\n' \"$i\" \"$arg\"\n  i=$((i + 1))\ndone\n",
        );
        let env = ScopedEnv::new(&temp);
        env.set(LOCALCODER_BIN_ENV, fake.as_os_str());
        env.set(LOCALCODER_ARGS_ENV, "chat --prompt {prompt} --mode 'text only'");

        let response = chat(LocalCoderChatRequest {
            prompt: "hello local coder".to_string(),
            timeout_ms: Some(5_000),
        })
        .await
        .unwrap();

        assert!(response.text.contains("argc=5"));
        assert!(response.text.contains("arg3=hello local coder"));
        assert!(response.text.contains("arg5=text only"));
        assert_eq!(response.strategy, LocalCoderCommandStrategy::EnvArgsPromptArg);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn status_reports_unavailable_without_install() {
        let temp = temp_dir("status-unavailable");
        let _env = ScopedEnv::new(&temp);

        let response = status().await;

        assert!(!response.available);
        assert!(response.bin_path.is_none());
        assert!(response
            .message
            .as_deref()
            .unwrap_or("")
            .contains("not found"));
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "trueos-localcoder-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_env<K, V>(key: K, value: V)
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        // Tests hold ENV_LOCK while mutating process-global env and cwd.
        unsafe { env::set_var(key, value) };
    }

    fn remove_env<K>(key: K)
    where
        K: AsRef<OsStr>,
    {
        // Tests hold ENV_LOCK while mutating process-global env and cwd.
        unsafe { env::remove_var(key) };
    }

    fn restore_env(key: &str, value: Option<&OsString>) {
        match value {
            Some(value) => set_env(key, value),
            None => remove_env(key),
        }
    }
}
