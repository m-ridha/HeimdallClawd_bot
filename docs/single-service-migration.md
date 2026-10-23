# Single-service migration

This release changes the trust model: native commands have internet access and
passwordless sudo root execution inside the credential-bearing agent container.
Every permitted account is a trusted operator. Root can change the bot and read
its credentials/state; pairing and private Telegram authorization remain required.
This document is a procedure, not evidence that production has been migrated.

## Resources

The single-service Trial proposal is a 2 shared-vCPU / 1,000,000,000-byte RAM
ceiling, one replica, sleeping disabled and the existing 500-MB /data volume.
Removing a service does not merge storage allowances. Trial allows a 0.5-GB volume,
1-GB ephemeral storage and 4-GB image; Free after Trial has lower compute ceilings.
The account's actual plan and Full/Limited Trial status need operator confirmation.
Limited Trial outbound restrictions cannot be bypassed by root or configuration.

Toolchains reside in image layers; project files and the Cargo cache use the
persistent volume. Large Rust builds or Chromium sessions may exceed the available
RAM/disk. Use one Cargo job and clean disposable caches; never purge databases,
sign-in or unknown files to free space. System installs made with sudo disappear
at container replacement; add durable OS dependencies to the Docker recipes.

References: [plans](https://docs.railway.com/pricing/plans),
[trial networking](https://docs.railway.com/pricing/free-trial),
[volumes](https://docs.railway.com/volumes/reference).

## Preserve files before retirement

1. Pause operator jobs and scheduled work that can write the old workspace.
   Use recovery mode for the main service if needed; do not clear event ledgers
   or replay reserved/uncertain email admissions.
2. Download a private backup of the agent volume and the old executor workspace
   outside this repository. Railway CLI 5.68.0 supports volume files download:
   inspect `railway volume files download --help`, select explicit project,
   environment and volume IDs, and download to a new private local directory.
   Do not print file contents or upload backups to Actions/release assets.
3. Verify download completeness and regular-file checksums; inspect symlinks,
   file modes, total bytes and names before importing. The destination is
   /data/zeroclaw/agents/primary/workspace, not the old /data/workspace.
   Preserve existing destination files and managed instructions; import first
   into a separate migration directory rather than overwriting collisions.
4. Upload the reviewed files using Railway volume files upload, or transfer a
   private verified archive and extract it through the service console. Do not
   extract arbitrary absolute/traversal paths. Restore service ownership (UID/GID
   1000) for imported project files only after checking their paths; do not
   recursively chown the whole volume or follow symlinks.
5. Verify checksums in the destination and keep the independent backup. Leave
   the retired volume intact until the new service and required files are tested.

## One-time infrastructure transition

The target IaC removes the old service but deliberately retains the old named
volume as unattached migration storage. This will produce destructive service/
attachment changes. The normal release workflow has no destructive override and
must not be weakened to force this through. Review and complete that one-time
maintenance after verified backup/import, before publishing the new release.
If the plan proposes volume deletion/shrinkage, stop and correct it.

Do not apply production infrastructure or delete live resources merely to test
this PR. After migration, retiring the retained volume is a separately reviewed
cleanup with explicit authorization and verified backups.

## Deploy and verify

Publish a new stable release only after the maintenance prerequisites and native
Linux validation succeed. Release packaging/deployment now requires only the
agent service ID. The existing sign-in, USER.md and session paths remain intact.
No backward-compatible executor endpoint, credentials or fallback is retained.

Verify Git/gh authentication without printing tokens, native read/write, internet
access, sudo -n id -u, all development-tool versions, browser navigation, Telegram
file round-trip, reminders, skills and SOP admission. Confirm state after restart.
Only then remove HEIMDALL_EXECUTOR_TOKEN/HEIMDALL_EXECUTOR_URL and stale native
workspace-MCP environment overrides from the agent, and the unused GitHub
production RAILWAY_EXECUTOR_SERVICE_ID variable. Avoid changing other secrets.

Never assume a failed/timeout mutation did nothing; inspect remote state before
retrying. A successful image build is not live migration or provider verification.
