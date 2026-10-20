# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# ClusterFuzzLite build image for the cargo-fuzz targets in fuzz/.
# Base image: OSS-Fuzz base-builder-rust (Apache-2.0), pinned by digest
# (manifest list of :latest on 2026-10-20). It brings its own nightly, clang
# and cargo-fuzz; nightly is used only for fuzzing. See docs/fuzzing.md.
FROM gcr.io/oss-fuzz-base/base-builder-rust@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
COPY . $SRC/galion
WORKDIR $SRC/galion
COPY .clusterfuzzlite/build.sh $SRC/
