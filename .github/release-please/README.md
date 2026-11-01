# Release preparation

This development-only tool uses Google's release-please (Apache-2.0) for reviewed release PRs
and changelogs. It is not included in the application or its JavaScript bundle.

The pinned adapter keeps Cargo's inherited workspace versions and both npm lockfile version
fields consistent. Test the actual installed adapter before updating release-please:

```
npm ci --ignore-scripts
npm test
```

Merging a release PR creates an unpublished private preview draft. CI and installer checks
must pass before assets are attached. A maintainer publishes the completed draft manually.
An existing unfinished draft pauses preparation of another version.
