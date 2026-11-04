# Norte Code provider plugins

This workspace contains provider plugins that can be installed with:

```text
norte-code plugins add https://github.com/JeanVydes/catalog-plugin --package catalog-plugin-openrouter
```

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
