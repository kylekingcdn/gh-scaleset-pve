# `gh-scaleset-pve`

GitHub Actions Runner Scale Set for Proxmox (in Rust)

The was inspired by this excellent write-up on the topic:
- [SysRoot - Ephemeral Self-Hosted CI Runners on Proxmox](https://sysroot.io/blog/github-actions-self-hosted-runners-on-proxmox-ephemeral)

## Features

- Built-in API to receive GitHub workflow job webhook events
- Advanced webhook event filtering, with restriction support
- Proxmox VM provisioning from template via PVE API
- Automatic cloud-init config generation for secure registration token handling
- Automatic pruning/reaping of VMs for completed and abandoned jobs
- Detailed logs outlining provisioning decisions
- OTEL support

## Deployment

Full documentation will be added shortly.

But as a brief summary, this is intended to **not** be deployed on the PVE host itself,
but rather in an LXC container or another machine.

Write access to the snippets directory used for cloud-init configs is required, as they
must be generated on a per-runner basis to handle injection of runner registration tokens.
- This can be done using:
  - A local path mount point on the LXC container
  - Exposing the snippets dir over NFS (through the PVE node or LXC), and mpunting the share
  - Or, if you do decide to run directly on Proxmox, just use the snippets directory path.
