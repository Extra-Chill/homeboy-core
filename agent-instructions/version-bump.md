Run a version bump / release workflow using **Homeboy**

1. run homeboy init to get your bearings
2. run homeboy changes <component> to understand changes to be documented
3. add changelog entries for each change since the last version bump using homeboy changelog add
4. run homeboy verson bump to bump the version 
5. run any tests using homeboy test
6. run homeboy release run to publish the release (if configured for this component)