FROM browser_base

RUN apt-get update \
    && apt-get install -y --no-install-recommends fluxbox novnc websockify x11vnc \
    && ln -sf vnc.html /usr/share/novnc/index.html \
    && rm -rf /var/lib/apt/lists/*

COPY browser/readiness.js /usr/local/bin/browser-readiness.js
COPY browser/write-chromium-flags.js /usr/local/bin/write-chromium-flags.js
COPY browser/graceful-stop.js /usr/local/bin/browser-graceful-stop.js
COPY browser/mark-profile-clean.js /usr/local/bin/mark-profile-clean.js
COPY --chmod=0755 browser/chromium-wrapper.sh /usr/local/bin/chromium-wrapper.sh
COPY --chmod=0755 browser/container-wrapper.sh /usr/local/bin/browser-container-wrapper.sh
COPY --chmod=0755 browser/x11vnc-wrapper.sh /usr/local/bin/x11vnc-wrapper.sh
COPY browser/chromium.conf /etc/supervisor/conf.d/services/chromium.conf
COPY browser/window-manager.conf /etc/supervisor/conf.d/services/mutter.conf
COPY browser/novnc.conf /etc/supervisor/conf.d/services/novnc.conf
