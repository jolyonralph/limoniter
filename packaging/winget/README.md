# Limoniter in winget

`winget install Limoniter` installs the same installer the Releases page has, once Limoniter is in Microsoft's
package list, github.com/microsoft/winget-pkgs.

The Release workflow makes the manifests for each release (the winget-manifests download), from
`make-manifests.py`. With a WINGET_TOKEN secret (a GitHub token that can make pull requests on public repositories),
it submits them as a pull request to winget-pkgs, where Microsoft's checks and a moderator take them in.

By hand: `python packaging/winget/make-manifests.py VERSION SHA256 out`, then `winget validate --manifest` the folder
it prints, and submit it with wingetcreate or a pull request.
