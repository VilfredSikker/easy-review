# The catalog owns the preset models

The models `er` ships live in `ai_hub_catalog.toml`. A user's config stores only what the catalog cannot rebuild: the user's own models, presets the user edited in Settings (marked `edited`), deletions of presets, and the default pick. Each load builds every unedited preset fresh from the catalog, and `save_config` never writes one back.

Before this, the first load copied every preset into the saved config, and from then on the copy won: the merge only added models that were missing. Each release's catalog changes stopped at that copy. Prices and labels stayed at the values of the release that first saved them, retired models stayed listed until someone added them to a hand-kept retirement table, and the model list kept the order presets were first merged in. In the desktop's ⌘K picker that put Haiku above Fable and Opus. Settings and ⌘K read the same in-memory hub, so both showed the copy, not the catalog.

## Consequences

- The one-time cost: configs saved before this have no `edited` mark, so an untouched copy and a hand-edited preset look the same. Both are replaced from the catalog on the first load. Models under the user's own ids are not touched.
- Removing a model from the catalog removes it everywhere on the next load. `RETIRED_PRESET_MODELS` remains for two jobs: moving a saved default to its successor, and dropping copies from configs written before this change, where an unknown id could equally be the user's own model.
- Saving a model from Settings marks it `edited`, and from then on the user's version wins over the catalog, even when it matches the catalog exactly. There is no reset to the shipped preset yet: deleting a preset records it in `removed_catalog_models`, so the catalog does not bring it back either.
- The user's own models are listed first, in saved order, then the presets in catalog order.
