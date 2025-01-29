The main crate of the game engine, containing code for platform-agnostic parts
of the runtime engine. Uses traits and types from
[platform-abstraction-layer](../platform-abstraction-layer) for
platform-specific functionality and types.

### Features

- `std`: enables safety measures related to panics. If not enabled, i.e.
  building for `#![no_std]`, `panic` should be set to `"abort"`, as the safety
  measures can't be used without `std`.
