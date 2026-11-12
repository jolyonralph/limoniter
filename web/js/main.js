// Limoniter's website: the cycling word in the heading, the machine picker, enlarged screenshots, the copy
// button and the narrow-screen menu. Everything still reads without it.

(() => {
  "use strict";

  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------- The heading's last word, typed and retyped ----------

  const words = ["PETSCII", "ATASCII", "EBCDIC", "Mode 7", "ZX Spectrum", "Amiga", "CP437", "Applesoft", "teletext", "8-bit"];
  const speaks = document.querySelector(".speaks");
  if (speaks && !reducedMotion) {
    let index = 0;
    const type = (word, then) => {
      let shown = 0;
      const step = () => {
        speaks.textContent = word.slice(0, shown);
        if (shown++ < word.length) setTimeout(step, 70);
        else then();
      };
      step();
    };
    const erase = (then) => {
      const step = () => {
        const text = speaks.textContent;
        if (text.length) {
          speaks.textContent = text.slice(0, -1);
          setTimeout(step, 35);
        } else then();
      };
      step();
    };
    const next = () => {
      setTimeout(() => erase(() => {
        index = (index + 1) % words.length;
        type(words[index], next);
      }), 2200);
    };
    next();
  }

  // ---------- Machines ----------

  const machines = [
    {
      name: "Commodore 64",
      blurb: "Also the VIC-20, PET, C128 and the 1541, 1571 and 1581 drives.",
      text: "PETSCII and screen codes, upper and lower case sets, with control codes written as {clr} and {red} as petcat writes them.",
      media: "D64, D71 and D81 disks and T64 tapes, with file names in PETSCII.",
      basic: "BASIC V2 programs (.prg, .p00) listed, and listings turned back into programs.",
      pictures: "Koala Painter, OCP and Advanced Art Studio, and Doodle!. PETSCII streams (.seq) are drawn with colour, reverse and cursor movement.",
      send: "A card or stick for an SD2IEC, Kung Fu Flash or Gotek. On an Ultimate 64 or 1541 Ultimate, a .prg starts running and a .d64 goes into drive A.",
    },
    {
      name: "ZX Spectrum",
      blurb: "48K, 128K, +3 and the Spectrum Next. The ZX80, ZX81 and QL character sets are here too.",
      text: "The Spectrum character set, with its colour and position controls shown as the screen would.",
      media: "TAP and TZX tapes, +3DOS DSK, TR-DOS and SCL disks, Microdrive cartridges, and Z80 and SNA snapshots.",
      basic: "48K, 128K and NextBASIC programs listed. Listings can be saved as a .tap, or as a Next .bas.",
      pictures: ".scr screens, with or without a +3DOS header.",
      send: "To a Spectrum Next over Wi-Fi: Limoniter serves the file, and .sync on the Next fetches it (NextSync).",
    },
    {
      name: "Amiga",
      blurb: "From Workbench floppies to hard disks and Amiga Forever packages.",
      text: "Amiga text, and Amiga ANSI with the console's own sequences. Syntax colouring for AmigaGuide, the Installer language, AMOS, Blitz Basic and 680x0 assembler.",
      media: "ADF floppies (OFS and FFS), ADZ, DMS, HDF hard disks with partitions, LHA archives and RP9 packages.",
      basic: "AMOS and Blitz Basic source is coloured as you edit.",
      pictures: "IFF ILBM and PBM, including HAM, HAM8, Extra Half-Brite and deep pictures, with Deluxe Paint colour cycling.",
      send: "A stick for a Gotek, a CF card, or a shared folder or FTP server on a networked Amiga.",
    },
    {
      name: "Atari 8-bit",
      blurb: "The 400, 800, XL and XE.",
      text: "ATASCII and screen codes, with 0x9B line endings kept exactly as they are.",
      media: "ATR and XFD disks in single, enhanced and double density, with Atari DOS 2, MyDOS or SpartaDOS. Text inside opens as ATASCII.",
      basic: "Atari BASIC programs listed.",
      pictures: "Text is shown in the machine's own character set.",
      send: "A TNFS server's folder for FujiNet, or a card for an SIO2SD or SDrive.",
    },
    {
      name: "Atari ST",
      blurb: "ST, STE and the GEM desktop.",
      text: "The Atari ST character set, separate from the 8-bit one.",
      media: ".st and packed .msa floppies.",
      basic: "STOS source is coloured as you edit.",
      pictures: "Degas, Degas Elite and NEOchrome, with colour cycling. GEM bit images (.img) too.",
      send: "A card for an ACSI2STM or UltraSatan, or a stick for a Gotek.",
    },
    {
      name: "BBC Micro and Acorn",
      blurb: "The BBC Micro, Electron, Atom and RISC OS machines.",
      text: "The BBC Micro character set, Mode 7 teletext with mosaics and double height, the Acorn Atom set and RISC OS's own.",
      media: "DFS (.ssd, .dsd), ADFS S, M and L, RISC OS ADFS D, E and F, and UEF tapes.",
      basic: "BBC BASIC II and IV listed, and listings turned back into programs.",
      pictures: "RISC OS sprite files, a single sprite or a whole sheet.",
      send: "A card for an MMFS or a Gotek, or a folder for an emulator.",
    },
    {
      name: "Apple II",
      blurb: "The Apple II family.",
      text: "High-bit ASCII as DOS 3.3 keeps text, with CR line endings.",
      media: "DOS 3.3 and ProDOS disks (.dsk, .do, .po, .2mg, .hdv) in either sector order, and 800K and hard disk volumes with folders.",
      basic: "Applesoft programs listed.",
      pictures: "Hi-res screens, in the colours an NTSC monitor gave them.",
      send: "A card for a Floppy Emu or a CFFA3000.",
    },
    {
      name: "Macintosh",
      blurb: "The classic Mac, System 1 to Mac OS 9.",
      text: "18 classic Macintosh encodings, Mac Roman among them, with CR line endings.",
      media: "MFS and HFS floppies, hard disks and CDs, Apple partition maps and Disk Copy 4.2 images. Both forks, and the type and creator, are shown.",
      basic: "Either fork of a file can be opened or saved out.",
      pictures: "MacPaint, with or without a MacBinary header.",
      send: "A folder shared with an emulator, or a card for a BlueSCSI.",
    },
    {
      name: "PC and DOS",
      blurb: "DOS, early Windows, and BBS-era art.",
      text: "20 DOS code pages (CP437, CP850, CP866 and more), and Windows and ISO code pages.",
      media: "FAT12, FAT16 and FAT32 floppy and hard disk images, ISO 9660 CDs with Joliet and Rock Ridge, ZIP archives, ImageDisk and Teledisk.",
      basic: "QBasic source coloured, and MBASIC programs listed.",
      pictures: "PC ANSI art with iCE colours and SAUCE, BinaryText, and ZSoft PCX.",
      send: "A stick or card for an old PC, a Gotek, or any folder.",
    },
    {
      name: "CP/M and TRS-80",
      blurb: "Kaypro, Osborne, Epson, Amstrad and the Tandy machines.",
      text: "CP/M text with Ctrl-Z kept, Kaypro and Osborne sets, the TRS-80 set and Amstrad CPC's.",
      media: "82 CP/M disk formats, worked out from the directory. TRS-80 disks with TRSDOS, LDOS and NEWDOS. Amstrad DSK with AMSDOS. CoCo and Dragon disks with RS-DOS, Dragon DOS and OS-9.",
      basic: "MBASIC, Color BASIC and Dragon BASIC listed.",
      pictures: "WordStar documents converted to Markdown, plain text or RTF.",
      send: "A card or a folder for an emulator, or a command line of your own.",
    },
    {
      name: "IBM mainframe",
      blurb: "z/OS data sets and EBCDIC files.",
      text: "22 EBCDIC code pages, with and without the euro. Text can have NL line endings, or be fixed 80-byte records.",
      media: "Files are opened as they came from the mainframe, byte for byte.",
      basic: "Syntax colouring for COBOL, JCL, PL/I, RPG, REXX and IBM HLASM.",
      pictures: "The hex view can show the bytes as EBCDIC.",
      send: "An SFTP or FTP server, written in the EBCDIC code page you choose on the way.",
    },
  ];

  const picker = document.querySelector(".machine-picker");
  const detail = document.querySelector(".machine-detail");
  const facts = [["text", "Text"], ["media", "Disks, tapes and archives"], ["basic", "Programs"], ["pictures", "Pictures and documents"], ["send", "Sending it back"]];

  const show = (n) => {
    const machine = machines[n];
    picker.querySelectorAll(".chip").forEach((chip, i) => {
      chip.setAttribute("aria-selected", String(i === n));
      chip.tabIndex = i === n ? 0 : -1;
    });
    detail.replaceChildren();
    const title = document.createElement("h3");
    title.textContent = machine.name;
    const blurb = document.createElement("p");
    blurb.className = "blurb";
    blurb.textContent = machine.blurb;
    const list = document.createElement("dl");
    list.className = "machine-facts";
    for (const [key, label] of facts) {
      const wrap = document.createElement("div");
      const dt = document.createElement("dt");
      dt.textContent = label;
      const dd = document.createElement("dd");
      dd.textContent = machine[key];
      wrap.append(dt, dd);
      list.append(wrap);
    }
    detail.append(title, blurb, list);
  };

  if (picker && detail) {
    machines.forEach((machine, n) => {
      const chip = document.createElement("button");
      chip.className = "chip";
      chip.type = "button";
      chip.role = "tab";
      chip.textContent = machine.name;
      chip.addEventListener("click", () => show(n));
      picker.append(chip);
    });
    // Arrow keys move between machines, as tabs do.
    picker.addEventListener("keydown", (event) => {
      const chips = [...picker.querySelectorAll(".chip")];
      const at = chips.indexOf(document.activeElement);
      if (at < 0) return;
      const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
      if (!step) return;
      event.preventDefault();
      const n = (at + step + chips.length) % chips.length;
      show(n);
      chips[n].focus();
    });
    show(0);
  }

  // ---------- The front page's screenshots, taking turns ----------

  const slideshow = document.querySelector(".slideshow");
  if (slideshow) {
    const slides = [...slideshow.querySelectorAll(".slide")];
    const caption = slideshow.querySelector(".slide-caption");
    const dots = slideshow.querySelector(".slide-dots");
    let current = 0;
    let timer = null;
    const go = (n) => {
      current = (n + slides.length) % slides.length;
      slides.forEach((slide, i) => {
        const on = i === current;
        slide.classList.toggle("active", on);
        slide.tabIndex = on ? 0 : -1;
        slide.setAttribute("aria-hidden", String(!on));
        // A picture not yet shown is fetched when its turn comes near.
        if (on || i === (current + 1) % slides.length) slide.querySelector("img").loading = "eager";
      });
      caption.textContent = slides[current].dataset.caption;
      [...dots.children].forEach((dot, i) => dot.setAttribute("aria-current", String(i === current)));
    };
    slides.forEach((slide, i) => {
      const dot = document.createElement("button");
      dot.type = "button";
      dot.setAttribute("aria-label", slide.dataset.caption);
      dot.addEventListener("click", () => {
        go(i);
        start();
      });
      dots.append(dot);
    });
    const stop = () => {
      clearInterval(timer);
      timer = null;
    };
    const start = () => {
      stop();
      if (!reducedMotion) timer = setInterval(() => go(current + 1), 4500);
    };
    // Held still while it's being looked at closely, or the page isn't in view.
    slideshow.addEventListener("mouseenter", stop);
    slideshow.addEventListener("mouseleave", start);
    slideshow.addEventListener("focusin", stop);
    slideshow.addEventListener("focusout", start);
    document.addEventListener("visibilitychange", () => (document.hidden ? stop() : start()));
    go(0);
    start();
  }

  // ---------- Screenshots, enlarged ----------

  const lightbox = document.querySelector(".lightbox");
  if (lightbox && typeof lightbox.showModal === "function") {
    const big = lightbox.querySelector("img");
    document.querySelectorAll(".shot, .slide").forEach((shot) => {
      shot.addEventListener("click", () => {
        const small = shot.querySelector("img");
        big.src = shot.dataset.full || small.src;
        big.alt = small.alt;
        lightbox.showModal();
      });
    });
    // A click outside the picture closes it.
    lightbox.addEventListener("click", (event) => {
      if (event.target === lightbox) lightbox.close();
    });
  }

  // ---------- Copy buttons ----------

  document.querySelectorAll(".copy").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(button.dataset.copy);
        button.textContent = "Copied";
      } catch {
        button.textContent = "Select and copy";
      }
      setTimeout(() => (button.textContent = "Copy"), 1800);
    });
  });

  // ---------- Narrow-screen menu ----------

  const menuButton = document.querySelector(".menu-button");
  const nav = document.getElementById("nav");
  if (menuButton && nav) {
    menuButton.addEventListener("click", () => {
      const open = nav.classList.toggle("open");
      menuButton.setAttribute("aria-expanded", String(open));
    });
    nav.addEventListener("click", (event) => {
      if (event.target.closest("a")) {
        nav.classList.remove("open");
        menuButton.setAttribute("aria-expanded", "false");
      }
    });
  }
})();
