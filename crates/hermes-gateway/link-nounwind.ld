/* SPDX-License-Identifier: MIT
   SPDX-FileCopyrightText: 3000 Galion / galion-studio
   Copyright (c) 3000 Galion / galion-studio

   Drop unwind tables from the x86_64 musl hermes-gateway image.
   Release builds use panic = abort, so these frames are never walked.
   Kept as its own script (INSERT) so lld's default musl script still runs.
*/
SECTIONS {
  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame_hdr)
    *(.gcc_except_table)
  }
}
INSERT BEFORE .text;
