#!/usr/bin/env bash
# CI-only: boot a checksum-pinned kernel 5.15 / glibc 2.35 image, without SSH.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'compatibility VM is a CI provisioning task' >&2; exit 2; }
image_url=https://cloud-images.ubuntu.com/releases/jammy/release-20261004/ubuntu-22.04-server-cloudimg-amd64.img
image_sha=012d81fade7e8ff4428fc35dfeee711d66d1f47d5ea9c5c8ced96a85b959835c
work=$(mktemp -d "${RUNNER_TEMP:?}/takt-compat.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
curl --fail --location --retry 3 --max-time 300 "$image_url" -o "$work/base.img"
printf '%s  %s\n' "$image_sha" "$work/base.img" | sha256sum -c -
qemu-img create -f qcow2 -F qcow2 -b "$work/base.img" "$work/guest.img" 5G
mkdir "$work/seed"
chmod +x dist/fixture-x86_64-*
cp dist/*x86_64-unknown-linux-*.tar.gz dist/*x86_64-unknown-linux-*.tar.gz.sha256 dist/fixture-x86_64-* "$work/seed/"
cp scripts/compatibility-guest.sh "$work/seed/smoke.sh"
cat > "$work/seed/meta-data" <<'EOF'
instance-id: takt-compatibility-20261004
local-hostname: takt-compatibility
EOF
cat > "$work/seed/user-data" <<'EOF'
#cloud-config
ssh_pwauth: false
disable_root: true
runcmd:
  - [bash, -c, 'mkdir -p /mnt/takt; mount -o ro /dev/sr0 /mnt/takt; if bash /mnt/takt/smoke.sh; then echo TAKT_COMPATIBILITY_PASS; else echo TAKT_COMPATIBILITY_FAIL; fi; poweroff']
EOF
genisoimage -quiet -output "$work/seed.iso" -volid cidata -joliet -rock "$work/seed"
acceleration=tcg
if [[ -r /dev/kvm && -w /dev/kvm ]]; then acceleration=kvm; fi
mkdir -p dist/compatibility
timeout --signal=TERM --kill-after=10s 600s qemu-system-x86_64 \
  -accel "$acceleration" -m 1024 -smp 2 -nographic -no-reboot \
  -drive "file=$work/guest.img,format=qcow2,if=virtio" \
  -drive "file=$work/seed.iso,media=cdrom,readonly=on" \
  -nic none 2>&1 | tee dist/compatibility/guest-serial.log
grep -q '^TAKT_COMPATIBILITY_PASS' dist/compatibility/guest-serial.log
! grep -q '^TAKT_COMPATIBILITY_FAIL' dist/compatibility/guest-serial.log
