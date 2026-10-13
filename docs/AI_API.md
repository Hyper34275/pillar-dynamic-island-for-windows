# Connecting Yuval to your own AI server

Since 1.0.17 smart search can send questions to an AI server the organisation runs or allows. It is **off by
default**: nothing is sent anywhere until `ai-config.json` says `"enabled": true` and names an endpoint.

Supported APIs:

| `provider` | Servers | Request |
|---|---|---|
| `openai` (default) | Anything OpenAI-compatible: vLLM, Ollama, LM Studio, LiteLLM, text-generation-inference, OpenAI | `POST …/chat/completions`, `Authorization: Bearer <key>` (no header when there is no key) |
| `azure` | Azure OpenAI | `POST …/openai/deployments/<model>/chat/completions?api-version=…`, `api-key: <key>` |
| `anthropic` | Anthropic Messages API, or a gateway in front of it | `POST …/v1/messages`, `x-api-key: <key>`, `anthropic-version: 2023-06-01` |

## Where to set it

Three ways, all writing the same file:

1. **Yuval Center → Settings → "חיבור לשרת AI"**: the switch, the server type, the address, the model, the key,
   when to ask, and buttons to save, test the connection and open the file.
2. **Tray menu → "הגדרות AI (קובץ CONFIG)"**: opens `ai-config.json` in Notepad (it is created with every key and a
   Hebrew explanation the first time). This works even where the Center cannot start.
3. **The file itself**:
   - `%LOCALAPPDATA%\Yuval\ai-config.json`: the user's settings;
   - `%ProgramData%\Yuval\ai-config.json`: an administrator's settings for every user of the PC. The user's file
     overrides it key by key, unless the machine file says `"locked": true` (then only the machine file counts and
     the Center shows the values read-only).

A change applies to the next question; nothing needs a restart.

## The file

```json
{
  "enabled": true,
  "provider": "openai",
  "endpoint": "http://ai-server:8000/v1",
  "apiKey": "",
  "model": "llama-3.1-70b-instruct",
  "mode": "fallback",
  "shareResults": true,
  "timeoutSeconds": 45,
  "maxTokens": 1024,
  "temperature": null,
  "systemPrompt": "",
  "headers": {},
  "apiVersion": "",
  "allowInvalidCertificates": false,
  "useWindowsAuth": false
}
```

| Key | Meaning |
|---|---|
| `enabled` | `true` to use the server. |
| `provider` | `openai`, `azure` or `anthropic` (see the table above). |
| `endpoint` | The server's address. A base (`http://host:8000/v1`, `https://host`) gets the API's path added; a full URL ending in `/chat/completions` or `/messages` is used as is. Azure: the resource (`https://x.openai.azure.com`) or the full deployment URL. |
| `apiKey` | The key, or `env:NAME` to read it from an environment variable. A key typed in the Center is stored as `apiKeyProtected` instead: encrypted with Windows DPAPI for this user, unreadable for anyone else. The key never reaches a log, a page or the Center. |
| `model` | The model name (Azure: the deployment name, unless it is in the URL). |
| `mode` | `fallback`: only questions the built-in engine did not understand. `always`: every question; the built-in search still runs and its results stay on the card (clickable), the AI writes the words above them. |
| `shareResults` | In mode `always`: send the results found (titles, times, senders, file names) with the question so the AI can answer about them. Off: the built-in answer is kept for those questions. |
| `timeoutSeconds` | 5–90 (default 45). |
| `maxTokens` | Upper bound of the answer's length (0 = not sent; Anthropic always gets 1024 then). |
| `temperature` | A number, or `null` for the server's default. |
| `systemPrompt` | Extra instructions appended to Yuval's own (date, language, brevity). |
| `headers` | Extra HTTP headers, e.g. `{ "X-Team": "it" }`; one with the same name as a built-in header replaces it. |
| `apiVersion` | Azure `api-version` when the endpoint does not carry one (default `2024-10-21`). |
| `allowInvalidCertificates` | `true` only for an internal server whose certificate this PC does not trust. Better: install the organisation's CA in Windows. |
| `useWindowsAuth` | `true` to answer a server's Negotiate/NTLM challenge with the signed-in Windows user. A proxy's challenge (407) is always answered that way. |

## What is sent, and what is not

- Only to the configured endpoint, only when a question is asked, through WinHTTP: the system's proxy settings
  (automatic proxy / PAC as Windows has them) and the Windows certificate store, TLS 1.2/1.3.
- The question, the last few questions and answers of the conversation (10 minutes, memory only), the date and
  time, and with `shareResults` in mode `always` the result lines described above. Never the mail bodies, file
  contents or attachments: Yuval does not read them.
- Never sent: requests to open, run or change something (they wait for a click as before), calculations, the
  help card, and requests Yuval refuses (delete, send, install, power).
- Logs (`logs\yuval.log`, category `ai`) carry the provider, the status, the duration and `AI-00x` codes; never
  the question, the answer, the endpoint or the key.

## Error codes

| Code | Meaning |
|---|---|
| `AI-001` | The settings are incomplete or the file is not valid JSON (the Center and the card say which). |
| `AI-002` | Cannot connect: name not found, connection refused, connection broken. Check the address, DNS, proxy, firewall. |
| `AI-003` | The server did not answer within `timeoutSeconds`. |
| `AI-004` | The server refused the key (HTTP 401/403), with the server's reason. |
| `AI-005` | Another HTTP error (404 wrong address or model, 429 busy or over quota, 5xx), with the server's reason. |
| `AI-006` | The answer is not in the expected format (or larger than 8 MB). |
| `AI-007` | The model declined, its filter blocked the answer, or it was empty. |
| `AI-008` | The server's TLS certificate is not trusted by this PC. |

When the AI does not answer, the card keeps the built-in answer and adds "שרת ה-AI לא ענה הפעם (AI-00x)".

## Code

- `src-tauri/src/llm/`: `config.rs` (the files, the merge, DPAPI), `protocol.rs` (URLs, headers, bodies, answers;
  pure, unit-tested), `http.rs` (WinHTTP), `mod.rs` (ask, test, the conversation, the Center's view).
- `src-tauri/src/assistant/ai.rs`: which questions go to the AI and how the answer becomes the card.
- Pipe commands `aiGetConfig`, `aiSetConfig`, `aiTest`, `aiOpenConfigFile` (Center settings page).
- A live test against a local mock server: `cargo test --lib llm::live -- --ignored --nocapture` with
  a server on `127.0.0.1:18080` (OpenAI, Azure and Anthropic paths).
