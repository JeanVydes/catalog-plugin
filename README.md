# Norte Code provider plugins

This workspace contains provider plugins that can be installed with:

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-openrouter
```

Build these plugins against Norte Code plugin ABI 4. Rebuild installed plugins when upgrading the host from an older ABI.

## OpenRouter

`catalog-plugin-openrouter` fetches `https://openrouter.ai/api/v1/models` when its
15-minute cache is missing or expired. The cache is stored at
`$XDG_CACHE_HOME/norte-code/plugins/openrouter-models.json`, or under
`~/.cache` when `XDG_CACHE_HOME` is not set. It exposes the OpenAI-compatible provider at
`https://openrouter.ai/api/v1` and maps chat-capable models into the host
catalogue. Models that only expose batch or image endpoints are not included.

The model listing is public, so no key is needed to discover it. Requests use
the normal Norte Code credential lookup and require `OPENROUTER_API_KEY`.

## models.dev

`catalog-plugin-models-dev` imports `https://models.dev/catalog.json?type=all` with a
15-minute cache. It keeps only providers using the OpenAI, OpenAI-compatible,
Anthropic, or Google AI SDK packages, and namespaces them as
`models-dev/<provider-id>` with names such as `Model.dev - Provider Name`.

Install it with:

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-models-dev
```

## Codex

`catalog-plugin-codex` reads the ChatGPT access token and account ID from
`$CODEX_HOME/auth.json` (or `~/.codex/auth.json`) and fetches the current model
catalogue from Codex's `https://chatgpt.com/backend-api/codex/models` endpoint
when the Norte Code service loads the plugin. It uses the Codex client version
from `models_cache.json`, falling back to `codex --version`, then includes only
models marked visible and supported by the API. The catalogue is account-aware
and is not copied into Norte Code's editable model files.

The endpoint is part of Codex's own backend and is not a documented public
catalogue API. Codex itself uses it, and the plugin follows the same response
shape, but OpenAI may change it without notice. The current ChatGPT access
token must be valid when Norte Code loads the plugin.

Install it with:

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-codex
```

When a Codex model is selected, the chat rail displays the account's available
usage windows. The plugin asks the installed, signed-in Codex CLI for limits
through its app server and refreshes while the rail is visible.

## OpenAI

`catalog-plugin-openai` reads the account-scoped model list from the official
`/v1/models` API using `OPENAI_API_KEY`. It keeps organization-owned model IDs
and excludes endpoint families that cannot be used with Norte Code's Chat
Completions wire, such as embeddings, image generation, audio, transcription,
and moderation. The API publishes model IDs and shutdown dates, but not model
capabilities or token limits, so the plugin does not claim image input,
reasoning, or context limits. Its 15-minute cache is scoped to a fingerprint of
the API key and never stores the key itself.

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-openai
```

## Google Gemini

`catalog-plugin-google` reads the Gemini API model list using `GOOGLE_API_KEY`
or `GEMINI_API_KEY`, follows `nextPageToken`, and keeps only models supporting
`generateContent`. It uses each model's `baseModelId` for requests and maps the
published token limits and `thinking` flag. The wire base is
`https://generativelanguage.googleapis.com/v1beta`. Its 15-minute cache is
scoped to a fingerprint of the API key and never stores the key itself.

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-google
```

## Anthropic

`catalog-plugin-anthropic` reads the Anthropic Models API using
`ANTHROPIC_API_KEY`, follows the `last_id` cursor, and excludes models marked
retired. It maps lifecycle, token limits, image and PDF input, structured
outputs, thinking, and reasoning efforts from the API's capability metadata.
Set `ANTHROPIC_WORKSPACE_ID` for an API key that is not scoped to one workspace;
the plugin includes it in both catalogue requests and the provider wire. Its
15-minute cache is scoped to a fingerprint of the API key and workspace, and
never stores either value.

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-anthropic
```
