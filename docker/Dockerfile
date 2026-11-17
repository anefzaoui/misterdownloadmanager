FROM debian:trixie-slim
ARG TARGETARCH

# The certificates downloads are checked against; nothing else (above).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /config /downloads && chown 1000:1000 /config /downloads

COPY bin/${TARGETARCH}/misterdownloadmanager bin/${TARGETARCH}/misterdownloadmanager-cli /usr/local/bin/

RUN chmod +x /usr/local/bin/misterdownloadmanager /usr/local/bin/misterdownloadmanager-cli

ENV MDM_DATA_DIR=/config MDM_DOWNLOADS=/downloads
VOLUME ["/config", "/downloads"]
EXPOSE 8463
USER 1000:1000
ENTRYPOINT ["misterdownloadmanager", "--headless"]
