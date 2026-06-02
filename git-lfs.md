# Working with `openings.epd` (Git LFS)

`openings.epd` is the default opening book (the **UHO_4060_v4** suite, ~15 MB,
241,670 positions). It is too large to store comfortably as a normal Git blob,
so it is tracked with **Git LFS** (Large File Storage). Git stores a tiny text
*pointer* in history; the real bytes live in LFS storage and are fetched on
checkout.

The small hand-curated book `openings-gambits.epd` (~2 KB) is **not** in LFS —
only `openings.epd` is.

## One-time setup on a new machine

LFS must be installed before you clone or pull, or you'll get pointer files
instead of the real data.

```sh
# macOS
brew install git-lfs

# Debian/Ubuntu
sudo apt-get install git-lfs

# Then, once per machine/user account:
git lfs install
```

## Cloning the repo

```sh
git clone <repo-url>
# Recent Git + LFS fetches LFS files automatically during checkout.
# If openings.epd looks like a 3-line pointer instead of FENs, run:
git lfs pull
```

## What a pointer looks like

If LFS is **not** set up, `openings.epd` will contain something like:

```
version https://git-lfs.github.com/spec/v1
oid sha256:4baf...
size 16226533
```

That means the real content hasn't been fetched — run `git lfs install` then
`git lfs pull`.

## Updating the opening book

LFS is transparent once configured: edit or replace the file and commit
normally. Because `.gitattributes` already routes `openings.epd` through LFS,
new content is stored in LFS automatically.

To refresh from upstream (e.g. a newer UHO release):

```sh
curl -sL -o /tmp/uho.zip \
  "https://github.com/official-stockfish/books/raw/master/UHO_4060_v4.epd.zip"
unzip -o /tmp/uho.zip -d /tmp/uho_out

# Re-add the attribution header, then the positions:
{
  echo '# UHO_4060_v4.epd - "Unbalanced Human Openings" by Stefan Pohl.'
  echo '# Source: https://github.com/official-stockfish/books (UHO_4060_v4.epd.zip)'
  cat /tmp/uho_out/UHO_4060_v4.epd
} > openings.epd

git add openings.epd
git commit -m "Update UHO opening book"
```

## Tracking additional large files

```sh
git lfs track "some-other-book.epd"   # appends a rule to .gitattributes
git add .gitattributes some-other-book.epd
git commit -m "Track some-other-book.epd with LFS"
```

Always commit the `.gitattributes` change in the **same or earlier** commit as
the file, so the file is captured by LFS from the start.

## Verifying

```sh
git lfs track          # show configured LFS patterns
git lfs ls-files       # list files actually stored in LFS (after committing)
git lfs status         # show staged/changed LFS objects
```

You should see `openings.epd` listed by `git lfs ls-files` once committed.

## Pushing

`git push` uploads LFS objects to the remote's LFS store automatically. The
remote must support LFS (GitHub, GitLab, Bitbucket, etc. do by default).

> First push of a large object can be slow — it uploads the full ~15 MB once.
> Subsequent clones download it from LFS rather than from Git history.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| File is a 3-line `version … oid … size …` pointer | `git lfs install` then `git lfs pull` |
| Want to clone *without* downloading LFS data | `GIT_LFS_SKIP_SMUDGE=1 git clone <url>`, fetch later with `git lfs pull` |
| `git lfs` command not found | Install git-lfs (see setup) |
| File committed as a normal blob by mistake | `git lfs migrate import --include="openings.epd"` (rewrites history — coordinate with collaborators) |

## Current state in this repo

`openings.epd` is already tracked (see `.gitattributes`) and **staged** as an
LFS object, but not yet committed. To commit it:

```sh
git commit -m "Add UHO opening book via Git LFS"
```
