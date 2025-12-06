# Fixes for the `polkadot-sdk`

- `sc-basic-authorship`: Has `sc-basic-authorship` check if an extrinsic was
  mandatory before checking if resources were exhausted. Then, if a mandatory
  extrinsic is ever flagged as exhausting resources, it panics as intended
  instead of being dropped as if non-mandatory.
