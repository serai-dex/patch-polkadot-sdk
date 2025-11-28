# Patches

None of these patches are guaranteed to work on their own, solely as
orchestrated by `patch.sh`. They may require further edits performed by
`patch.sh` or require other patches present.

These patches primarily prioritize _stability_. They should not be affected by
the addition/removal of other patches, nor by updating the `polkadot-sdk`
version derived from. If they are broken, they should be quick to correct.

File removals are preferred to be done via `rm`, not via patch files, as `git`
will consider updates to removed files as conflicts when the purpose is to
remove them entirely.

- `features`: Features added on top.

- `fixes`: Bug fixes.

- `opinions`: Tweaks to behavior per Serai's opinions.

- `removals`: Patch to remove references to removed crates/functionality.

- `std`: Patches which replace external dependencies with usage of the standard
  library.

- `updates`: Patches to update dependencies across major versions, resolving
  breaking changes.
