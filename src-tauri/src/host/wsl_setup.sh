# Brings a WSL distribution in line for sharing, changing only what is
# missing: a second run changes nothing. Runs as root inside the distribution
# and expects set_ini and ini_missing (wsl_ini.sh) and these variables:
#   DN_USER            the Linux user that gets Docker
#   DN_PORT            the port sshd listens on
#   DN_WSLCONFIG       the Windows user's .wslconfig as a WSL path, or empty
#   DN_MEMORY          memory= for .wslconfig when the user chose one (8GB), or empty
#   DN_MEMORY_DEFAULT  memory= only when .wslconfig has none yet
# Every change prints "dockernanny-changed: <what>"; the caller restarts WSL
# only for the changes that need it.
set -e
export DEBIAN_FRONTEND=noninteractive
changed() { echo "dockernanny-changed: $1"; }

# Packages, only the missing ones. apt-daily holds apt's lock for a while
# after the first boot with systemd, so apt waits for it instead of failing.
need=""
if ! command -v sshd >/dev/null 2>&1 && [ ! -x /usr/sbin/sshd ]; then need="$need openssh-server"; fi
if ! command -v rsync >/dev/null 2>&1; then need="$need rsync"; fi
if ! command -v docker >/dev/null 2>&1; then
  if ! command -v curl >/dev/null 2>&1; then need="$need curl"; fi
  if [ ! -f /etc/ssl/certs/ca-certificates.crt ]; then need="$need ca-certificates"; fi
fi
if [ -n "$need" ]; then
  if ! command -v apt-get >/dev/null 2>&1; then
    echo "This distribution has no apt-get. Install$need with its own package manager, then run Set up again." >&2
    exit 1
  fi
  apt-get -o DPkg::Lock::Timeout=180 update -qq
  # shellcheck disable=SC2086
  apt-get -o DPkg::Lock::Timeout=180 install -y -qq $need
  changed "installed$need"
fi

# Docker Engine through Docker's own install script, and only when there is
# no docker at all: an existing install is never touched.
if ! command -v docker >/dev/null 2>&1; then
  curl -fsSL https://get.docker.com | sh
  changed "installed Docker Engine"
fi
if ! id -nG "$DN_USER" | tr ' ' '\n' | grep -qx docker; then
  usermod -aG docker "$DN_USER"
  changed "added $DN_USER to the docker group"
fi

# sshd on its own port through a drop-in; the distribution's config stays as it is.
sshd_port_changed=no
dropin=/etc/ssh/sshd_config.d/dockernanny.conf
if [ "$(cat "$dropin" 2>/dev/null)" != "Port $DN_PORT" ]; then
  mkdir -p /etc/ssh/sshd_config.d
  printf 'Port %s\n' "$DN_PORT" > "$dropin"
  sshd_port_changed=yes
  changed "sshd listens on $DN_PORT"
fi
# Ubuntu's socket activation listens on 22 whatever sshd's config says, so
# the plain service is used when the socket is on.
if systemctl is-enabled ssh.socket >/dev/null 2>&1; then
  systemctl disable --now ssh.socket >/dev/null 2>&1 || true
  changed "sshd runs as a service"
fi
for unit in ssh docker; do
  if ! systemctl is-enabled "$unit" >/dev/null 2>&1; then
    systemctl enable "$unit" >/dev/null 2>&1 || true
    changed "$unit starts with the distribution"
  fi
done

# systemd starts sshd and Docker with the distribution; the user is who logs in.
if set_ini /etc/wsl.conf boot systemd true; then changed "wsl.conf: systemd on"; fi
if set_ini /etc/wsl.conf user default "$DN_USER"; then changed "wsl.conf: $DN_USER is the default user"; fi

# The Windows side: mirrored networking, and no idle shutdown of the distro
# or the VM. memory is set when the user chose it, or when there is none yet.
if [ -n "$DN_WSLCONFIG" ]; then
  wslconfig_changed=no
  if set_ini "$DN_WSLCONFIG" wsl2 networkingMode mirrored; then wslconfig_changed=yes; fi
  if set_ini "$DN_WSLCONFIG" wsl2 vmIdleTimeout -1; then wslconfig_changed=yes; fi
  if set_ini "$DN_WSLCONFIG" general instanceIdleTimeout -1; then wslconfig_changed=yes; fi
  if [ -n "$DN_MEMORY" ]; then
    if set_ini "$DN_WSLCONFIG" wsl2 memory "$DN_MEMORY"; then wslconfig_changed=yes; fi
  elif ini_missing "$DN_WSLCONFIG" wsl2 memory; then
    if set_ini "$DN_WSLCONFIG" wsl2 memory "$DN_MEMORY_DEFAULT"; then wslconfig_changed=yes; fi
  fi
  if [ "$wslconfig_changed" = yes ]; then changed ".wslconfig"; fi
fi

# With systemd already running, what changed takes effect now, no restart.
if [ -d /run/systemd/system ]; then
  if [ "$sshd_port_changed" = yes ]; then systemctl restart ssh; fi
  systemctl is-active --quiet ssh || systemctl start ssh
  systemctl is-active --quiet docker || systemctl start docker
fi
echo dockernanny-linux-ok
