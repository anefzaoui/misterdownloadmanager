# Mister Download Manager on a server, without a window (#157): docs/server.md.
#
#   docker build -t misterdownloadmanager .
#   docker run -d --name mdm -p 8463:8463 -v mdm-config:/config -v /srv/downloads:/downloads \
#     misterdownloadmanager
#   docker logs mdm                                  # the address and a pairing code
#   docker exec mdm misterdownloadmanager-cli pair   # another code
#
# PROFILE=release (the default) is what ships: slow to build (whole-program optimisation);
# PROFILE=vmtest-like speed comes from `--build-arg PROFILE=dev` for a quick try.
ARG PROFILE=release

FROM rust:1-trixie AS build
ARG PROFILE
# The libraries the desktop app links to (as CI's Linux job); the server never opens a window.
RUN apt-get update \
 && apt-get install -y --no-install-recommends cmake pkg-config libgtk-3-dev libxkbcommon-dev \
      libwayland-dev libgl1-mesa-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --locked --profile "$PROFILE" -p mdm-app -p mdm-cli \
 && out="target/$PROFILE"; [ "$PROFILE" = dev ] && out=target/debug; \
    mkdir -p /out && cp "$out/misterdownloadmanager" "$out/misterdownloadmanager-cli" /out/ \
 && strip /out/*

FROM debian:trixie-slim
# The certificates downloads are checked against, and the libraries the program loads.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libgtk-3-0t64 libxkbcommon0 \
      libgl1 libegl1 \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /config /downloads && chown 1000:1000 /config /downloads
COPY --from=build /out/ /usr/local/bin/
# The list, settings, certificate and paired devices in /config; the files in /downloads.
ENV MDM_DATA_DIR=/config MDM_DOWNLOADS=/downloads
VOLUME ["/config", "/downloads"]
# The remote control (HTTPS); torrents use a port of their own (Torrent settings).
EXPOSE 8463
# Any user works (`--user 99:100` on Unraid): it needs only to write /config and /downloads.
USER 1000:1000
ENTRYPOINT ["misterdownloadmanager", "--headless"]
