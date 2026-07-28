# Off-site backups

Dara can continuously copy its databases and media to a private Cloudflare R2
bucket. This protects against losing the Mac or its drive. It is optional:
Dara remains local-first and works without an account or network connection.

This is backup, not sync. Do not open the same backup set from two active Dara
installations. A replacement Mac must explicitly take ownership before it can
write new backups.

## What is protected

Dara backs up:

- cards, reviews, settings, and the rest of the relational database;
- the media database and every active media blob; and
- a small checkpoint record that says which database and media versions form a
  complete, restorable backup.

Dara does not back up:

- the downloaded embedding model or its derived search vectors, which Dara can
  recreate;
- local logs, caches, or temporary files;
- R2 credentials stored in the macOS Keychain; or
- application binaries.

The R2 objects are private by default, but they are not encrypted by Dara
before upload. Cloudflare can therefore process the stored data as part of
providing R2. Use a dedicated private bucket and credentials limited to that
bucket.

## Set up Cloudflare R2

1. In Cloudflare, create a private R2 bucket using the **Standard** storage
   class. Do not enable public access.
2. Create an R2 API token with **Object Read & Write** permission, restricted
   to that one bucket.
3. Record the following values while Cloudflare shows them:

   - Account ID
   - Bucket name
   - Access Key ID
   - Secret Access Key
   - Jurisdiction (`Default` unless you deliberately chose a restricted
     jurisdiction)

4. Choose a backup prefix. A value such as `dara/personal-v1` keeps Dara's
   objects in one namespace inside the bucket.

Cloudflare documents [creating S3-compatible credentials][r2-tokens],
[R2's S3 endpoint][r2-s3], and [jurisdiction-specific endpoints][r2-location].
Bucket names are permanent, so choose the bucket intentionally.

[r2-tokens]: https://developers.cloudflare.com/r2/api/tokens/
[r2-s3]: https://developers.cloudflare.com/r2/get-started/s3/
[r2-location]: https://developers.cloudflare.com/r2/reference/data-location/

## Enable backup in Dara

Open **Settings → Off-site backup**, enter the R2 values, and choose **Save and
test connection**. Dara stores the access key and secret in the macOS Keychain,
not in either SQLite database.

After the connection succeeds, enable off-site backup. The settings screen
reports three related pieces of progress:

- **Database copy** means Litestream is continuously shipping database changes
  to R2.
- **Media copy** means Dara has uploaded the image and other media files needed
  by active cards.
- **Complete backup** means Dara has published a checkpoint tying a particular
  database state to its complete media set. Only complete checkpoints are
  offered for recovery.

An interrupted upload is safe. Dara resumes its work and does not label a
backup complete until all required pieces are present.

## How fresh is the backup?

While backup is healthy, Litestream tries to send relational changes every five
seconds. That alone is not a complete Dara backup. Dara normally starts a
complete-checkpoint attempt after one quiet minute; continuous changes force an
attempt after five minutes. Media and network time can make completion take
longer.

If Settings reports pending media, the newest database state may refer to an
image that has not reached R2 yet. The previous complete checkpoint remains the
recovery point until that media arrives and a new checkpoint finishes. Treat
the displayed **last complete backup** time—not database freshness—as the
answer to “how much could I lose?”

These timings are intentionally fixed because they jointly control correctness,
request volume, and cost. They are not tuning knobs in Settings.

## Retention and storage growth

Dara configures Litestream to retain 30 days of relational recovery history.
Remote media is append-only in this first release: deleting a card or local
blob does not automatically delete its older R2 object. That favors recovery
safety over reclaiming every byte, so stored media can grow over time.

Dara offers only complete checkpoints whose exact relational transaction and
required media can still be restored. Deleting individual R2 objects by hand
can invalidate an otherwise useful checkpoint.

## Prove that recovery works

After the first complete backup appears, use **Run restore drill** in Settings.
The drill downloads the latest complete checkpoint into a temporary,
repository-independent location, opens and validates both databases, and checks
the required media. It does not replace the live Dara data.

A recent successful drill is stronger evidence than a green “connected”
message: it proves that Dara can read and assemble the stored backup.

## Recover after losing the Mac

1. Install the same or a newer compatible Dara release on the replacement Mac.
2. Open the recovery flow and enter the same R2 account, bucket, jurisdiction,
   and prefix. The R2 credentials must be entered again because Keychain
   credentials are machine-local and are intentionally not backed up.
3. Inspect and restore the latest complete checkpoint.
4. Check important cards and media before deleting or changing any old backup
   data.
5. If the old installation is permanently gone, explicitly **Take over backup
   ownership**. Dara creates a new ownership era before the replacement Mac can
   publish new checkpoints.

The takeover step prevents two Macs from silently writing incompatible backup
histories to the same prefix. Do not take over merely to move between two
currently active computers; off-site backup is not multi-device sync.

## Disable or decommission backup

These are separate actions:

- **Disable backup** stops new uploads. Existing R2 objects and Keychain
  credentials remain.
- **Remove credentials** deletes Dara's R2 credentials from the local Keychain.
  It does not delete remote objects.
- **Delete the backup** is a manual Cloudflare operation. First disable backup,
  run any final recovery check you need, then delete only the dedicated Dara
  prefix or bucket in R2.

Never add an R2 lifecycle rule or Object Lock policy that deletes, transitions,
expires, or prevents cleanup of Dara objects unless it has been tested against
the restore protocol. A rule can make a checkpoint appear present while
silently removing data it needs, or prevent test/decommission cleanup.

R2 pricing and minimum-storage rules can change. Check Cloudflare's current
[pricing][r2-pricing] and [storage-class documentation][r2-storage] instead of
relying on a cost copied into this repository.

[r2-pricing]: https://developers.cloudflare.com/r2/pricing/
[r2-storage]: https://developers.cloudflare.com/r2/buckets/storage-classes/

## Developer credentials

Local development may load credentials from the ignored `app/.env.local`.
Commit only `app/.env.example`, which contains names and safe placeholders.
Never put live credentials in source, fixtures, logs, acceptance evidence, or
GitHub Actions configuration.

The scheduled/dispatch-only R2 canary uses credentials from the protected
`r2-canary` GitHub environment. It creates a unique prefix for each run,
performs a real upload and restore, then deletes only that prefix.
