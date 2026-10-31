// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Library half of galion-one (2026-10-31): the Approvals screen module
//! ([`approvals`]), shared by the galion-one binary (one.galion.app
//! `/approvals`) and the console (`console-static --phone`, the phone
//! owner home on console.galion.app). Same files, same checks, same
//! owner key; neither process ever deploys.

pub mod approvals;
