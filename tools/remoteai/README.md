# remoteAI

`remoteai` is a Linux-hosted Rust facade for the GitHub Copilot CLI runtime.
It accepts the small OpenAI Chat Completions subset used by the Dobby
Blueprint and returns either one function tool call or a plain summary.
With `parallel_tool_calls: true`, it can return one ordered batch of up to eight
function calls while preserving the original OpenAI tool-call shape.

The Rust Copilot SDK bundles the matching CLI runtime, so TRUEOS does not need
Node, Python, the Copilot CLI, or GitHub credentials. The host process starts
the bundled CLI over JSON-RPC in `ClientMode::Empty`: no shell, filesystem,
MCP, skills, local tools, Lumen, or text-to-speech capabilities are exposed.
The SDK and bundled runtime retain their separate terms; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Build and run

Build and run one command from the repo:

```sh
cd tools/remoteai
cargo build --release
./target/x86_64-unknown-linux-gnu/release/remoteai
```
`cargo run --release` also uses the same flow.

For TRUEOS LAN testing, bind the host's private address (for this machine,
`192.168.178.111:3042`) and host the HTTP facade on this LAN.
This path is plaintext HTTP and is deliberately test-only.

The tracked [Dobby template](examples/dobby-config.remoteai.json) can be
uploaded as `apps/dobby/config.json` and uses the local `192.168.178.111:3042`
endpoint by default. Leave `api_key` empty for local private HTTP usage.

The real Dobby config is ignored by git.

## User service

Install the binary and service template; each start prints:

```sh
install -d -m700 ~/.local/state/remoteai
install -Dm755 target/x86_64-unknown-linux-gnu/release/remoteai ~/.local/bin/remoteai
install -Dm644 systemd/remoteai.service ~/.config/systemd/user/remoteai.service
systemctl --user daemon-reload
systemctl --user enable --now remoteai.service
systemctl --user status --no-pager remoteai.service
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

GitHub Copilot Free supports only automatic model selection, so leave
`model=auto`. The five-second autonomous mode consumes GitHub AI credits continuously;
it is intended for bounded bring-up tests rather than an unlimited background loop.

## HTTP contract

- `GET /healthz` is unauthenticated and returns service liveness.
- `GET /v1/models` is available on LAN and requires no token in anonymous mode.
- `POST /v1/chat/completions` is available on LAN and accepts non-streaming JSON.

Message `content` may be a string or an OpenAI-style content-parts array. Text
parts and at most two `image_url` parts are accepted. Images must be inline
`data:image/png;base64,...` URLs; remote URLs and other media types are
rejected. Decoded PNG data is capped at 80 KiB in aggregate so the request
remains compatible with TRUEOS's bounded JSON-POST path. The facade forwards
accepted images only as in-memory Copilot SDK blob attachments and never
writes them to the host filesystem.

Each completion uses an ephemeral Copilot session. Dobby remains responsible
for its serialized queue, ten ordinary turns, rollover summary, reset logic,
and execution of screen-spirit or UI tools. With `parallel_tool_calls: false`,
the facade preserves the original behavior: it captures one external tool
request and immediately aborts the Copilot turn. With
`parallel_tool_calls: true`, only one private synthetic batch tool is exposed
to Copilot; the facade validates and expands its ordered `calls` array into one
to eight calls using only the caller's original allowlist. The synthetic tool
name never appears in the OpenAI response. In both modes the facade never
executes a tool. Tool results and any follow-up inference request remain owned
by Dobby.
