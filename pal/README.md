A crate containing shared traits and types between the engine crate and the
platform implementation crates. The main trait is Pal (short for "platform
abstraction layer"), which is used via a &dyn in the engine, and is implemented
by platform implementations.
