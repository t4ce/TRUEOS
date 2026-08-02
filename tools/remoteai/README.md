# remoteAI

`remoteai` is a Linux-hosted Rust facade for the GitHub Copilot CLI runtime.
It accepts the small OpenAI Chat Completions subset used by the Dobby
Blueprint and returns either one function tool call or a plain summary.

The Rust Copilot SDK bundles the matching CLI runtime, so TRUEOS does not need
Node, Python, the Copilot CLI, or GitHub credentials. The host process starts
the bundled CLI over JSON-RPC in `ClientMode::Empty`: no shell, filesystem,
MCP, skills, local tools, Lumen, or text-to-speech capabilities are exposed.
The SDK and bundled runtime retain their separate terms; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Build and run

The host must already be eligible for a GitHub Copilot plan. `remoteai` can use
the active `gh` account directly. If that account is not picked up, run the
bundled runtime's device flow without installing another CLI:

```sh
cargo run --release -- login
```

Then build and serve:

```sh
cd tools/remoteai
cargo build --release
export REMOTEAI_BIND=127.0.0.1:3042
export REMOTEAI_BEARER_TOKEN='replace-with-a-long-random-secret'
./target/x86_64-unknown-linux-gnu/release/remoteai serve
```

For TRUEOS LAN testing, bind the host's private address (for this machine,
`192.168.178.111:3042`). This path is plaintext HTTP and is deliberately
test-only: keep it on a trusted LAN and use a unique bearer token. HTTPS
remains Dobby's default.

The tracked [Dobby template](examples/dobby-config.remoteai.json) can be
uploaded as `apps/dobby/config.json` after replacing its bearer placeholder.
The real Dobby config is ignored by git.

## User service

Install the binary and service template, then let the Rust binary create a
fresh untracked bearer secret without printing it:

```sh
install -d -m700 ~/.local/state/remoteai
install -Dm755 target/x86_64-unknown-linux-gnu/release/remoteai ~/.local/bin/remoteai
install -Dm644 systemd/remoteai.service ~/.config/systemd/user/remoteai.service
REMOTEAI_BIND=192.168.178.111:3042 ~/.local/bin/remoteai init
systemctl --user daemon-reload
systemctl --user enable --now remoteai.service
loginctl enable-linger "$USER"
```

The linger setting keeps the user service running across logout and starts it
at host boot. Omit that line if `remoteai` should exist only while the Ubuntu
desktop user is logged in.

If Ubuntu's firewall is enabled, admit only the trusted TRUEOS LAN rather than
opening the facade globally:

```sh
sudo ufw allow from 192.168.178.0/24 to any port 3042 proto tcp comment 'remoteAI for TRUEOS'
```

Build and publish the Dobby Blueprint from the Blueprint repository with
`cargo bp dobby`. On TRUEOS, invoke it with `online dobby` or `§§dobby`; the
VMX minishell is supplied automatically.

Copy only the generated `REMOTEAI_BEARER_TOKEN` value into Dobby's `api_key`.
Do not put a GitHub token in the Dobby file; it stays on the Linux host only.
`init` refuses to overwrite an existing environment file.

GitHub Copilot Free supports only automatic model selection, so leave
`REMOTEAI_MODEL=auto`. The five-second autonomous mode consumes GitHub AI
credits continuously; it is intended for bounded bring-up tests rather than an
unlimited background loop.

## HTTP contract

- `GET /healthz` is unauthenticated and returns service liveness.
- `GET /v1/models` requires the bearer token.
- `POST /v1/chat/completions` requires the bearer token and non-streaming JSON.

Each completion uses an ephemeral Copilot session. Dobby remains responsible
for its serialized queue, ten ordinary turns, rollover summary, reset logic,
and execution of the three screen-spirit tools. On ordinary turns the facade
captures the first external tool request and immediately aborts the Copilot
turn; it never executes the tool and does not spend a second inference call on
a tool result.
