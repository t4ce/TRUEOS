# Apply the prepared change

Prepared against branch `true` at commit
`725095bfcbe5a159feb4731e9ee118eb838a9d6f`.

```bash
git switch true
git pull --ff-only
git switch -c feat/gna-audio-front-end
git apply --check gna-audio-frontend.patch
git apply gna-audio-frontend.patch
git diff --check
git status --short
```

Then run the repository's normal formatter/build path, commit, push, and use
`PR_BODY.md` as the pull-request description.
