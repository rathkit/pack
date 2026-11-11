#!/usr/bin/env bash
# Makes the apt mirror of a GitHub-hosted Ubuntu runner reachable before apt is used.
#
# The runner's mirror list (/etc/apt/apt-mirrors.txt) puts azure.archive.ubuntu.com first, and from the runner that name often does not
# resolve (on a test run, 4 of 10 lookups did; the others, archive.ubuntu.com and security.ubuntu.com, resolved every time). apt then
# fetches some packages from that one host only, waits about 20 seconds per failed lookup, and gives up (run 43045111211, three
# attempts, "Temporary failure resolving 'azure.archive.ubuntu.com'"). The fallback mirrors work but are far slower (about 14 kB/s
# against 4 MB/s), so the fast mirror is kept when it can be reached:
#   1. resolve the name, trying up to 15 times, 2 seconds apart;
#   2. when it resolves, pin the address in /etc/hosts, so apt does not depend on the flaky lookup for the rest of the job;
#   3. when it never does, drop the name from the mirror list and use the slower mirrors.
# Packages stay signed by Ubuntu's keys whichever mirror serves them; only the host that serves the files changes.
# APT_MIRROR_HOST and APT_MIRRORS_FILE exist so the fallback can be tried without waiting for a failure.
set -u
host=${APT_MIRROR_HOST:-azure.archive.ubuntu.com}
list=${APT_MIRRORS_FILE:-/etc/apt/apt-mirrors.txt}

if ! grep -q -- "$host" "$list" 2>/dev/null; then
  echo "ci-apt-mirror: $host is not in $list, nothing to do"
  exit 0
fi

ip=
for try in $(seq 1 15); do
  ip=$(timeout 10 getent ahostsv4 "$host" | awk 'NR == 1 { print $1 }')
  if [ -n "$ip" ]; then
    echo "ci-apt-mirror: $host resolved to $ip on try $try"
    break
  fi
  echo "ci-apt-mirror: $host did not resolve (try $try of 15)"
  sleep 2
done

if [ -n "$ip" ]; then
  [ -n "${APT_MIRRORS_FILE:-}" ] || echo "$ip $host" | sudo tee -a /etc/hosts >/dev/null
  echo "ci-apt-mirror: pinned $host to $ip"
else
  sudo sed -i "\\|$host|d" "$list"
  echo "ci-apt-mirror: $host never resolved, removed from $list; apt will use the other mirrors"
fi
