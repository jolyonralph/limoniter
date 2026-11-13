# Limoniter in the Microsoft Store

The Store signs the package and installs it without SmartScreen's warning, and keeps it up to date.

## Once

1. Register as an individual developer at https://storedeveloper.microsoft.com (free; a Microsoft account, and an
   ID and selfie to check it's you).
2. In Partner Center, under Apps and games, make a new app and reserve the name Limoniter.
3. Under the app's Product identity, copy Package/Identity/Name, Package/Identity/Publisher and
   Package/Properties/PublisherDisplayName into the GitHub repository's Actions variables, as STORE_NAME,
   STORE_PUBLISHER and STORE_PUBLISHER_NAME (Settings, Secrets and variables, Actions, Variables).

## Each release

1. Push the release's tag as usual. The Release workflow builds the Store package too: it's the store-package
   download on the workflow run's page.
2. In Partner Center, start a submission: upload the .msix under Packages, fill in or keep the Store listing
   (description, screenshots), and submit. The first one is reviewed by hand, which takes a few days.

To make the package on this computer: `packaging\windows-store\make-msix.ps1`, after `installer\build-installer.ps1`.
Without the variables above set (as LIMONITER_STORE_NAME and so on), it uses stand-ins, to look at but not to upload.

From the Store, Limoniter's Settings, File types sends people to Windows' Default apps, since a packaged app's file
types are declared by its package (file-types.txt).
