#!/usr/bin/env bash
# Read-only runner evidence: CPU inventory does not establish a cgroup quota.
set -euo pipefail

date --utc --iso-8601=ns
printf '/proc/self/cgroup\n'
cat /proc/self/cgroup
found=0
while IFS=: read -r hierarchy controllers relative; do
  if [[ "$hierarchy" != 0 || -n "$controllers" ]]; then continue; fi
  found=1
  if [[ "$relative" != /* ]]; then
    echo 'Invalid cgroup v2 path: expected an absolute path.' >&2
    exit 1
  fi
  directory="$(realpath -m -- "/sys/fs/cgroup$relative")"
  case "$directory" in
    /sys/fs/cgroup | /sys/fs/cgroup/*) ;;
    *) echo 'Refusing a cgroup path outside /sys/fs/cgroup.' >&2; exit 1 ;;
  esac
  while true; do
    printf '\nCgroup: %s\n' "$directory"
    for name in cpu.max cpu.stat cpuset.cpus.effective; do
      file="$directory/$name"
      printf '%s\n' "$file"
      if [[ -r "$file" ]]; then cat "$file"; else echo '(not readable in this mount namespace)'; fi
    done
    if [[ "$directory" == /sys/fs/cgroup ]]; then break; fi
    directory="${directory%/*}"
  done
done < /proc/self/cgroup
if [[ "$found" == 0 ]]; then
  echo 'No cgroup v2 membership was reported; no CPU quota is inferred.'
fi
