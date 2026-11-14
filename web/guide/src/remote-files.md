description: Open, edit and save files on a server over SFTP, open a file from a web address, and send files to FTP and SFTP servers on your machines.

# Remote files

Limoniter can open a file straight from a server over SFTP, edit it, and save it back, and it can fetch a file from a web address. This page shows you how.

## Opening a file on a server

1. Choose **File › Open a remote file (SFTP)…**.
2. Type the server's address, such as `sftp://user@host/folder` (add `:port` after the host if it isn't the usual one).
3. Choose a **Key file** to sign in with, or leave it empty to sign in with a password. Type the password if one is needed, and tick **Remember it** if you'd like it kept.
4. Click **Connect**.
5. Click folders to go into them, and double-click a file to open it.

The first time you connect to a server, its key's fingerprint is shown. Trust it only if it's what the server's owner says it should be, and then click **Trust it and connect**. After that, a server whose key has changed is refused, in case someone is in between.

> A kept password goes in your system's own store (Credential Manager on Windows, the keychain on a Mac, the keyring on Linux), never in Limoniter's settings.

## Editing and saving

A remote file is edited in a copy on this computer, so everything works on it, very large files included. Each time you save, the file is sent back to the server in the background, to a new file beside the old one first, so the server's copy is never half-written.

- If someone else has changed the file on the server since you opened it, Save asks before replacing it.
- If sending fails, your changes are still saved on this computer, and the message says where.

## Saving any file to a server

To put a file you're editing on a server:

1. Choose **File › Save to a server (SFTP)…**.
2. Connect as above, and go to the folder you'd like it in.
3. Give it a **File name**, and click **Save here**.

## Opening a file from the web

To open a file from a web address:

1. Choose **File › Open a file from the web (http/https)…**.
2. Type or paste its address, such as `https://example.com/file.txt`.
3. Click **Open**.

The file is fetched and opened as any file is: a disk image in the browser, text in its character set. It has no file of its own until you save it where you choose.

## Sending to a machine's server

Many machines can be reached over the network: a MiSTer or a Raspberry Pi over SFTP, or an Ultimate 64 or a networked Amiga over FTP. To send files to one, set it up once as a device under **File › Send to device › Manage devices…**, with the kind **SFTP server** or **FTP server**. The server's key is checked as for remote files. See [Sending to a device](assembling-and-running.html#sending-to-a-device).
