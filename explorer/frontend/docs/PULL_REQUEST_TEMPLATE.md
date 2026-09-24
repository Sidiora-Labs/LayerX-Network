## Description and Related Issue(s)

*[Provide a brief description of the changes or enhancements introduced by this pull request and explain motivation behind them. Cite any related issue(s) or bug(s) that it addresses using the [format](https://blog.github.com/2013-05-14-closing-issues-via-pull-requests/) `Fixes #123` or `Resolves #456`.]*

### Proposed Changes
*[Specify the changes or additions made in this pull request. Please mention if any changes were made to the ENV variables]*

### Breaking or Incompatible Changes
*[Describe any breaking or incompatible changes introduced by this pull request. Specify how users might need to modify their code or configurations to accommodate these changes.]*

### Additional Information
*[Include any additional information, context, or screenshots that may be helpful for reviewers.]*

## Checklist for PR author
- [ ] I have tested these changes locally.
- [ ] I added tests to cover any new functionality, following this [guide](./CONTRIBUTING.md#writing--running-tests)
- [ ] Whenever I fix a bug, I include a regression test to ensure that the bug does not reappear silently.
- [ ] If I have added a feature or functionality that is not privacy-compliant (e.g., tracking, analytics, third-party services), I have disabled it for private mode.
- [ ] If I have added, changed, renamed, or removed an environment variable
    - I updated the list of environment variables in the [documentation](ENVS.md) 
    - I made the necessary changes to the validator script according to the [guide](./CONTRIBUTING.md#adding-new-env-variable)
    - I added "ENVs" label to this pull request
