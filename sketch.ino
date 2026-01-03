#include <WiFi.h>
#include <Adafruit_NeoPixel.h>
#include <cstring>
#include <mbedtls/sha1.h>
#include <mbedtls/base64.h>
#include <pgmspace.h>

#define LINE_LEN 2048

static char g_serialLine[LINE_LEN];

Adafruit_NeoPixel pixels(1, 48, NEO_GRB + NEO_KHZ800);
WiFiServer server(80);
WiFiClient wsClient;
bool wsActive = false;

const char INDEX_HTML[] PROGMEM = R"rawliteral(
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Log2Website Console</title>
  <style>
    :root {
      --bg-grad: radial-gradient(circle at 20% 20%, #1f1b2c, #090c16 60%);
      --card-bg: rgba(15, 18, 30, 0.85);
      --accent: #6ee7ff;
      --accent-strong: #ff7edb;
      --text: #f5f6ff;
      --page-gap: 2rem;
      font-family: "Space Grotesk", "Fira Sans", sans-serif;
    }
    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg-grad);
      padding: 0;
      color: var(--text);
      display: flex;
      justify-content: center;
      align-items: center;
    }
    .card {
      box-sizing: border-box;
      background: var(--card-bg);
      border-radius: 24px;
      padding: 2rem;
      margin: var(--page-gap);
      width: calc(100vw - 2 * var(--page-gap));
      height: calc(100vh - 2 * var(--page-gap));
      display: flex;
      flex-direction: column;
      gap: 1.5rem;
      box-shadow: 0 15px 40px rgba(0, 0, 0, 0.4);
      border: 1px solid rgba(255, 255, 255, 0.1);
      backdrop-filter: blur(8px);
    }
    h1 {
      margin: 0 0 1rem;
      font-size: clamp(1.8rem, 3vw, 2.6rem);
      letter-spacing: 0.04em;
    }
    .status {
      font-size: 1rem;
      margin-bottom: 1rem;
      display: inline-flex;
      align-items: center;
      gap: 0.5rem;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    .status::before {
      content: "";
      width: 0.75rem;
      height: 0.75rem;
      border-radius: 999px;
      background: var(--accent);
      animation: pulse 1.2s infinite;
    }
    .status.streaming::before { background: var(--accent-strong); }
    .status.reconnecting::before { background: #ffb347; }
    .status.error::before { background: #ff5f6d; }
    @keyframes pulse {
      0% { opacity: 0.2; transform: scale(0.8); }
      50% { opacity: 1; transform: scale(1); }
      100% { opacity: 0.2; transform: scale(0.8); }
    }
    pre {
      margin: 0;
      background: rgba(0, 0, 0, 0.35);
      border-radius: 16px;
      padding: 1.5rem;
      font-size: 0.95rem;
      line-height: 1.5;
      flex: 1 1 auto;
      min-height: 0;
      overflow-y: auto;
      white-space: pre-wrap;
      word-break: break-word;
      border: 1px solid rgba(255, 255, 255, 0.08);
      box-shadow: inset 0 0 30px rgba(0, 0, 0, 0.35);
    }
  </style>
</head>
<body>
  <div class="card">
    <h1>Serial Console Bridge</h1>
    <div id="status" class="status reconnecting">Connecting…</div>
    <pre id="log">Waiting for inbound console logs…</pre>
  </div>
  <script>
    (() => {
      const statusEl = document.getElementById('status');
      const logEl = document.getElementById('log');
      let ws = null;
      let lines = [];

      function updateStatus(text, stateClass) {
        statusEl.textContent = text;
        statusEl.className = `status ${stateClass}`;
      }

      function appendLine(line) {
        if (!line) return;
        const marginOfError = 4;
        const distanceFromBottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight;
        const isPinnedToBottom = distanceFromBottom <= marginOfError;
        const retainFromBottom = logEl.scrollHeight - logEl.scrollTop;
        lines.push(line);
        if (lines.length > 500) {
          lines = lines.slice(-500);
        }
        logEl.textContent = lines.join('\n');
        if (isPinnedToBottom) {
          logEl.scrollTop = logEl.scrollHeight;
        } else {
          const target = logEl.scrollHeight - retainFromBottom;
          logEl.scrollTop = target < 0 ? 0 : target;
        }
      }

      function ensureConnection() {
        if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
          return;
        }
        updateStatus('Connecting…', 'reconnecting');
        ws = new WebSocket(`ws://${window.location.host}/ws`);
        ws.onopen = () => {
          appendLine('Waiting for inbound console logs…');
          updateStatus('Connected · waiting for inbound logs', 'waiting');
        };
        ws.onmessage = (event) => {
          if (lines.length && lines[lines.length - 1].includes('Waiting for inbound')) {
            lines = [];
          }
          appendLine(event.data);
          updateStatus('Streaming console logs', 'streaming');
        };
        ws.onerror = () => {
          updateStatus('WebSocket error · retrying', 'error');
          ws.close();
        };
        ws.onclose = () => {
          updateStatus('Disconnected · retrying…', 'reconnecting');
        };
      }

      setInterval(ensureConnection, 250);
      ensureConnection();
    })();
  </script>
</body>
</html>
)rawliteral";

static void sendWsLine(WiFiClient &client, const char *msg) {
  size_t len = strlen(msg);
  const size_t maxLen = 65500; // safety cap
  if (len > maxLen) {
    len = maxLen;
  }

  uint8_t header[4];
  size_t hlen = 0;
  header[0] = 0x81; // FIN + text frame
  if (len < 126) {
    header[1] = static_cast<uint8_t>(len);
    hlen = 2;
  } else {
    header[1] = 126;
    header[2] = static_cast<uint8_t>((len >> 8) & 0xFF);
    header[3] = static_cast<uint8_t>(len & 0xFF);
    hlen = 4;
  }

  client.write(header, hlen);
  client.write(reinterpret_cast<const uint8_t *>(msg), len);
}

static bool computeWebSocketAccept(const char *key, char *out, size_t outLen) {
  const char guid[] = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
  char concat[128];
  snprintf(concat, sizeof(concat), "%s%s", key, guid);

  uint8_t sha[20];
  mbedtls_sha1(reinterpret_cast<const unsigned char *>(concat), strlen(concat), sha);

  size_t olen = 0;
  if (mbedtls_base64_encode(reinterpret_cast<unsigned char *>(out), outLen, &olen, sha, sizeof(sha)) != 0) {
    return false;
  }
  if (olen < outLen) {
    out[olen] = '\0';
  } else if (outLen > 0) {
    out[outLen - 1] = '\0';
  }
  return true;
}

static void closeWebSocket() {
  if (wsActive) {
    wsClient.stop();
  }
  wsActive = false;
}

static void streamSerialLine(const char *line) {
  if (!wsActive) return;
  if (!wsClient.connected()) {
    closeWebSocket();
    return;
  }
  sendWsLine(wsClient, line);
}

static void discardWsInput() {
  if (!wsActive || !wsClient.connected()) return;
  while (wsClient.available()) {
    wsClient.read();
  }
}

static void serveIndexPage(WiFiClient &client) {
  size_t htmlLen = strlen_P(INDEX_HTML);
  client.println("HTTP/1.1 200 OK");
  client.println("Content-Type: text/html; charset=utf-8");
  client.println("Cache-Control: no-store, no-cache, must-revalidate");
  client.print("Content-Length: ");
  client.println(htmlLen);
  client.println("Connection: close");
  client.println();
  client.print(FPSTR(INDEX_HTML));
}
void setup() {
  pixels.begin();
  pixels.clear();
  pixels.setPixelColor(0, pixels.Color(255, 85, 255));pixels.show();
  Serial.begin(921600);
  Serial.println("Log2Website");
  WiFi.setHostname("Log2Website");
  WiFi.begin("easyeasy", "easyeasy!");
  while (WiFi.status() != WL_CONNECTED) {
    delay(100);
  }
  Serial.println(WiFi.localIP());
  pixels.setPixelColor(0, pixels.Color(0, 255, 0));pixels.show();
  server.begin();
}
void loop() {
  if (wsActive && !wsClient.connected()) {
    closeWebSocket();
  }
  discardWsInput();

  if (Serial.available()) {
    pixels.setPixelColor(0, pixels.Color(255, 255, 255));pixels.show();
    size_t readLen = Serial.readBytesUntil('\n', g_serialLine, LINE_LEN - 1);
    g_serialLine[readLen] = '\0';

    bool truncated = (readLen == LINE_LEN - 1 && Serial.peek() != '\n');
    if (truncated) {
      while (Serial.available()) {
        if (Serial.read() == '\n') break;
      }
    }

    while (readLen > 0 && (g_serialLine[readLen - 1] == '\r' || g_serialLine[readLen - 1] == '\n')) {
      g_serialLine[--readLen] = '\0';
    }

    if (truncated) {
      const char truncMarker[] = " [TRUNC]";
      size_t markerLen = sizeof(truncMarker) - 1;
      size_t insertAt = strlen(g_serialLine);
      if (insertAt + markerLen >= LINE_LEN) {
        insertAt = LINE_LEN - 1 - markerLen;
      }
      memcpy(&g_serialLine[insertAt], truncMarker, markerLen);
      g_serialLine[insertAt + markerLen] = '\0';
    }

    streamSerialLine(g_serialLine);
  }
  WiFiClient client = server.available();
  if (client) {
    pixels.setPixelColor(0, pixels.Color(255, 0, 0)); pixels.show();
    unsigned long waitStart = millis();
    while (client.connected() && !client.available()) {
      if (millis() - waitStart > 2000) {
        client.stop();
        return;
      }
      delay(1);
    }

    bool isWebSocket = false;
    char wsKey[64] = {0};
    bool wantsWsPath = false;

    // Read request line
    char line[256];
    size_t len = client.readBytesUntil('\n', line, sizeof(line) - 1);
    line[len] = '\0';
    while (len > 0 && (line[len - 1] == '\r' || line[len - 1] == '\n')) {
      line[--len] = '\0';
    }
    if (len >= 7 && strncmp(line, "GET /ws", 7) == 0) {
      wantsWsPath = true;
    }

    // Read headers
    while (true) {
      len = client.readBytesUntil('\n', line, sizeof(line) - 1);
      line[len] = '\0';
      while (len > 0 && (line[len - 1] == '\r' || line[len - 1] == '\n')) {
        line[--len] = '\0';
      }
      if (len == 0) break; // blank line

      if (strncasecmp(line, "Sec-WebSocket-Key:", 18) == 0) {
        const char *v = line + 18;
        while (*v == ' ') v++;
        strncpy(wsKey, v, sizeof(wsKey) - 1);
      } else if (strncasecmp(line, "Upgrade:", 8) == 0 && strstr(line, "websocket")) {
        isWebSocket = true;
      }
    }

    if (wantsWsPath && isWebSocket && wsKey[0] != '\0') {
      char acceptKey[64];
      if (computeWebSocketAccept(wsKey, acceptKey, sizeof(acceptKey))) {
        if (wsActive) {
          closeWebSocket();
        }
        client.println("HTTP/1.1 101 Switching Protocols");
        client.println("Upgrade: websocket");
        client.println("Connection: Upgrade");
        client.print("Sec-WebSocket-Accept: ");
        client.println(acceptKey);
        client.println();

        wsClient = client;
        wsActive = true;
        return;
      }
    }

    serveIndexPage(client);
    client.stop();
  }
}
