#!/usr/bin/env bash
set -euo pipefail

: "${SPACEKIT_BIN:?set SPACEKIT_BIN to the spacekit binary}"
: "${CLUSTER_ROOT:?set CLUSTER_ROOT to the cluster directory}"

configure_node() {
  local node="$1"
  local offset="$2"
  local home="$CLUSTER_ROOT/node-$node"

  set_key() {
    HOME="$home" "$SPACEKIT_BIN" network config set "$1" "$2"
  }

  set_key node_id "node-$node"
  set_key ports.storage_http "$((3030 + offset))"
  set_key ports.storage_p2p "$((4001 + offset))"
  set_key ports.compute_http "$((9000 + offset))"
  set_key ports.compute_p2p "$((9001 + offset))"
  set_key ports.messaging_listen "$((7100 + offset))"
  set_key ports.messaging_bootstrap "$((7000 + offset))"
  set_key ports.messaging_http "$((17000 + offset))"
  set_key ports.gateway_http "$((8080 + offset))"
  set_key ports.status_http "$((9100 + offset))"
  set_key ports.keymaster_coordinator "$((8780 + offset))"
  set_key ports.keymaster_registry "$((8770 + offset))"
  set_key ports.keymaster_guardian_base "$((8781 + offset))"
  set_key urls.storage "http://127.0.0.1:$((3030 + offset))"
  set_key urls.compute "http://127.0.0.1:$((9000 + offset))"
  set_key urls.gateway "http://127.0.0.1:$((8080 + offset))"
  set_key urls.keymaster_coordinator "http://127.0.0.1:$((8780 + offset))"
  set_key urls.keymaster_registry "http://127.0.0.1:$((8770 + offset))"
  set_key messaging.listen_addr "0.0.0.0:$((7100 + offset))"
}

configure_node a 0
configure_node b 20000
configure_node c 40000

for node in a b c; do
  HOME="$CLUSTER_ROOT/node-$node" "$SPACEKIT_BIN" network config show >/dev/null
done
