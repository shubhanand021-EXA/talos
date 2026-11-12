# Talos ephemeral computer-use desktop (Alpine + XFCE + x11vnc).
#
# Minimal GUI surface for the agent: a lightweight XFCE session on DISPLAY :0,
# x11vnc publishing the root window on 5900 (loopback-published by the Talos
# manager only when VNC viewing is enabled), xdotool for input injection,
# and ImageMagick `import` for screenshots. A non-root `agent` user owns
# /home/agent (writable layer, discarded with the container); /workspace is
# mounted read-only by the manager (never baked in); verified artifacts leave
# only via `computer.export_file` from /home/agent/output.
FROM alpine:3.21

RUN apk add --no-cache \
    xfce4 \
    xfce4-terminal \
    xfce4-screenshooter \
    xvfb \
    x11vnc \
    xdotool \
    imagemagick \
    ttf-dejavu \
    dbus-x11 \
    sudo \
 && adduser -D -s /bin/sh agent \
 && mkdir -p /workspace /home/agent/output \
 && chown -R agent:agent /home/agent \
 && chmod 755 /workspace \
 && echo 'agent ALL=(ALL) NOPASSWD: /usr/bin/x11vnc' >> /etc/sudoers

ENV DISPLAY=:0 \
    HOME=/home/agent \
    XDG_RUNTIME_DIR=/tmp/runtime-agent

USER agent
WORKDIR /home/agent

# Xvfb virtual display, XFCE session, then x11vnc on 5900 (no password:
# the port is only ever published on 127.0.0.1 by the Talos manager).
CMD ["sh", "-c", "mkdir -p $XDG_RUNTIME_DIR && Xvfb :0 -screen 0 1280x800x24 >/tmp/xvfb.log 2>&1 & sleep 2 && startxfce4 >/tmp/xfce.log 2>&1 & sleep 3 && exec x11vnc -display :0 -forever -shared -nopw -listen 127.0.0.1 -rfbport 5900"]

HEALTHCHECK --interval=10s --timeout=5s --retries=12 \
    CMD sh -c 'xdotool getdisplaygeometry >/dev/null 2>&1'
