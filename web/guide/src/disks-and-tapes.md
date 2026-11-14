description: Open disk images, tapes and archives from older computers as folders, take files out of them, put files on them, and make new blank images.

# The disk and tape browser

Limoniter opens the disk images, tapes and archives of older computers as if they were folders: Commodore D64s, Spectrum TAP files, Amiga ADFs, Apple II disks, CP/M disks, Macintosh HFS volumes, ZIP and StuffIt archives, and many more. This page shows you how to look inside them, take files out, and change the images Limoniter can write.

The [full feature list](../features.html#disk-tape-and-archive-browser) has every format, machine by machine.

## Opening an image

Open the image as you'd open any file, with **File › Open…** ([[Ctrl+O]]) or by dropping it on the window. It opens as a browser tab, listing the files on it.

![Fred Fish disk 801 in the disk browser](amiga-disk "An Amiga floppy from the Fred Fish collection, opened as a folder.")

- To open a file, folder or an image inside the image, double-click it or press [[Enter]].
- To go up a folder, click **↑ Up** or press [[Backspace]].
- To see everything the image says about itself, and about the chosen file, click **ℹ Info**. It shows in the File properties panel.
- To look at the image's own bytes, click **Show the image in hex**.

A file opens in a tab of its own, read-only, so everything works on it: previews, BASIC listings, pictures and character sets. Commodore file names are shown in PETSCII, and Spectrum BASIC from tapes, disks and snapshots comes out as a +3DOS file.

> An image is never changed just by opening it. Archives, and the kinds of image Limoniter doesn't write, are only ever read. A damaged or unexpected image gives an error, never a wrong file.

## Taking a file out

To copy a file off an image onto your computer:

1. Select the file in the browser.
2. Click **Save a copy…**.
3. Choose where to put it. It's written out exactly as it is on the image.

A picture in an older machine's format can be saved as a PNG instead, with **Save as PNG…**, and a WordStar or HP MemoMaker document can be opened as plain text with **Open as text**.

To run a program straight from an image, select it and press [[F9]]. See [Assembling and running](assembling-and-running.html).

## Changing an image

Some kinds of image can be changed: files added, deleted and renamed, folders made, and a tape's files put in a different order.

| Image | What you can put on it |
|---|---|
| ZX Spectrum TAP and TZX tapes | Programs (a listing typed as text is tokenised), code at an address, screens, headerless blocks, or every block of another tape |
| Spectrum +3 and Amstrad CPC DSK disks | Files, with a +3DOS header put on a program, code or screen, or kept as they are |
| Commodore D64, D71 and D81 | PRG, SEQ and USR files; PC64 `.P00` files are unwrapped |
| PC and Atari ST FAT12, FAT16 and FAT32 | Files and folders, with long names |
| Amiga ADF (OFS and FFS, DD and HD) | Files and folders |
| Atari 8-bit ATR with Atari DOS 2.0 and 2.5 | Files, on single, enhanced and double density disks |
| Color Computer RS-DOS (JVC) | BASIC, data, machine code at an address, and text |

Other images, such as MyDOS and SpartaDOS disks, are only read, and say why they're read only.

### Adding files

1. Click **➕ Add files…** and choose the files, or drop them from your file manager onto the browser tab.
2. Where it matters, the Add panel says what each file will go on as (a program, code at an address, a SEQ file…). Change it if you need to, and check the name it will have on the image.
3. If a file of that name is already there, it's kept unless you tick **Replace it**.
4. Click **Add**.

### Renaming, deleting and making folders

- To rename a file, select it and click **Rename** (or press [[F2]]).
- To take a file off the image, select it and click **Delete** (or press [[Delete]]).
- On an image with folders, click **New folder**.
- On a tape, the **▲** and **▼** buttons move the selected file earlier or later.

### Undoing a change

Each change is saved over the image straight away. The image is written whole and read back before it replaces the old one, so a change never leaves it half-written. If you change your mind, click **↶ Undo**: it puts the image back as it was before the change, for each of the last 20.

> Keep a copy of any image that matters to you before changing it, as you would with any other file.

## Making a new image

To make a blank disk or tape, formatted as its machine would format it:

1. Choose **File › New disk or tape image…**.
2. Choose the kind of image, and give the disk a name if you like.
3. Click **Save as…** and choose where to keep it.

It opens in the browser, ready for files to be added.

## Images from disk archivists

Limoniter also reads the images people make when they save old floppies: ImageDisk, Teledisk, DMK, JV3 and JV1 images, and flux images from a Greaseweazle, SuperCard Pro or KryoFlux. It decodes them, opens the file system inside, and lists any sectors that never read cleanly. A decoded flux image can be saved as a plain disk image.
