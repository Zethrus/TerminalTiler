FROM debian:12

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    build-essential \
    ca-certificates \
    curl \
    file \
    libadwaita-1-dev \
    libgraphene-1.0-dev \
    libgtk-4-dev \
    libvte-2.91-gtk4-dev \
    pkg-config \
    python3 \
    wget \
    xz-utils \
 && rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

RUN wget -q https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -O /usr/local/bin/appimagetool \
 && chmod +x /usr/local/bin/appimagetool
