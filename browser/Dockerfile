FROM browser_base

RUN apt-get update \
    && apt-get install -y --no-install-recommends fluxbox novnc websockify x11vnc \
    locales \
    fonts-nanum \
    fonts-noto-cjk \
    fontconfig \
    && sed -i '/^# *ko_KR.UTF-8 UTF-8/d; /^ko_KR.UTF-8 UTF-8/d; /^# *en_US.UTF-8 UTF-8/d; /^en_US.UTF-8 UTF-8/d' /etc/locale.gen \
    && printf 'en_US.UTF-8 UTF-8\nko_KR.UTF-8 UTF-8\n' >> /etc/locale.gen \
    && locale-gen \
    && ln -sf vnc.html /usr/share/novnc/index.html \
    && perl -0pi -e 's@        var text = this\._sock\.rQshiftStr\(length\);@        var raw = this._sock.rQshiftStr(length);\n        var rawBytes = new Uint8Array(raw.length);\n        for (var i = 0; i < raw.length; i++) { rawBytes[i] = raw.charCodeAt(i); }\n        var text = new TextDecoder("utf-8").decode(rawBytes);@' /usr/share/novnc/core/rfb.js \
    && perl -0pi -e 's|<link rel="stylesheet" href="app/styles/base.css" />|<link rel="stylesheet" href="app/styles/base.css" />\\n    <link rel="stylesheet" href="app/styles/novnc-style-overrides.css" />|' /usr/share/novnc/vnc.html \
    && sed -i 's/text\.charCodeAt(i)/(textBytes[i] === undefined ? textBytes.charCodeAt(i) : textBytes[i])/g' /usr/share/novnc/core/rfb.js \
    && sed -i 's/        var n = text.length;/        var textBytes = new TextEncoder().encode(text);\n        var n = textBytes.length;/' /usr/share/novnc/core/rfb.js \
    && sed -i 's#\.\./core/rfb\.js#../core/rfb.js?cokacdir_clipboard_v4#' /usr/share/novnc/app/ui.js \
    && sed -i 's#src="app/ui.js"#src="app/ui.js?cokacdir_clipboard_v4"#' /usr/share/novnc/vnc.html \
    && rm -rf /var/lib/apt/lists/*

COPY browser/browser-readiness.js /usr/local/bin/browser-readiness.js
COPY --chmod=0755 browser/browser-renderer-watchdog.js /usr/local/bin/browser-renderer-watchdog.js
COPY browser/write-chromium-flags.js /usr/local/bin/write-chromium-flags.js
COPY browser/graceful-stop.js /usr/local/bin/browser-graceful-stop.js
COPY browser/mark-profile-clean.js /usr/local/bin/mark-profile-clean.js
COPY --chmod=0755 browser/chromium-wrapper.sh /usr/local/bin/chromium-wrapper.sh
COPY --chmod=0755 browser/container-wrapper.sh /usr/local/bin/browser-container-wrapper.sh
COPY --chmod=0755 browser/x11vnc-wrapper.sh /usr/local/bin/x11vnc-wrapper.sh
COPY browser/chromium.conf /etc/supervisor/conf.d/services/chromium.conf
COPY browser/window-manager.conf /etc/supervisor/conf.d/services/mutter.conf
COPY browser/novnc.conf /etc/supervisor/conf.d/services/novnc.conf
COPY browser/fonts.conf /etc/fonts/local.conf
COPY browser/gtk-settings.ini /etc/gtk-3.0/settings.ini
COPY browser/policy.json /etc/chromium/policies/managed/policy.json
COPY browser/gtk2rc /etc/gtk-2.0/gtkrc
COPY browser/Xresources /home/kernel/.Xresources
COPY browser/fluxbox /root/.fluxbox
COPY browser/fluxbox /home/kernel/.fluxbox
COPY browser/novnc-style-overrides.css /usr/share/novnc/app/styles/novnc-style-overrides.css
