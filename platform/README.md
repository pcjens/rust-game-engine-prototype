<!--
SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>

SPDX-License-Identifier: GPL-3.0-or-later
-->

A crate containing shared traits and types between [engine](../engine) and the
platform implementation crates (e.g. [platform-sdl2](../platform-sdl2)).

The main trait is `Pal` (short for "platform abstraction layer"), which is used
by platform-agnostic parts of the game engine, and implemented by the platform
implementations. For cases where the platform needs to call the engine, the
engine has an analogous trait as well, `EngineCallbacks`.
