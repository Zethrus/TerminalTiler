#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  appstream \
  build-essential \
  dbus-x11 \
  dpkg-dev \
  libadwaita-1-dev \
  libgraphene-1.0-dev \
  libgtk-4-dev \
  libjavascriptcoregtk-6.0-dev \
  libsoup-3.0-dev \
  libvte-2.91-gtk4-dev \
  libwebkitgtk-6.0-dev \
  pkg-config \
  shellcheck \
  wget \
  xvfb
