; ch15_load_picture.s - loads a picture from disk while the demo runs: a copper bar keeps bouncing, a progress
; bar fills, and the picture appears a band at a time as it arrives.
;
; How To Code v8, chapter 15. CC0: use it for anything.
;
; With the system running only (OS_FRIENDLY 1): loading a file needs dos.library, and dos.library needs the
; system - its file systems, the disk's device, interrupts, multitasking. Built with OS_FRIENDLY 0 it ends at once.
;
; It loads ch15_picture.raw (made by ch15_make_picture.py, beside it) from the current directory: put the two
; files in the same place and start it from there. v8's test copies both into the emulated Amiga's DH0, and also
; runs it from a floppy disk, which the system reads with the help of the blitter (trackdisk.device does, on
; Kickstart 1.x, and on a 68000 even in AmigaOS 3.2).
;
; $VER: ch15_load_picture 8.0 (20.11.2026)

OS_FRIENDLY     equ     1

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
PLANES          equ     4
LINEBYTES       equ     ROWBYTES*PLANES ; interleaved: a line of each bitplane, one after another
SCREENSIZE      equ     LINEBYTES*HEIGHT
PICTURETOP      equ     16              ; the picture's first line on the screen
PICTURELINES    equ     240
PICTURESIZE     equ     LINEBYTES*PICTURELINES
COLOURS         equ     16
CHUNKLINES      equ     16              ; how much is read each frame
BARTOP          equ     4               ; the progress bar: its lines on the screen
BARLINES        equ     8
COPPERBARLINES  equ     8               ; the bouncing copper bar
COPPERBARTOP    equ     $40
COPPERBARBOTTOM equ     $f0

MODE_OLDFILE    equ     1005            ; Open: a file that's there already (dos/dos.i)

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ifeq    OS_FRIENDLY
        ; Taking the machine, nothing can be loaded: end at once.
        st      ExitNow
        rts
        else
        move.l  #LoaderExit,ExitHook

        ; The screen, from the system's chip RAM: MEMF_CHIP because the chips show it, MEMF_CLEAR so it starts
        ; empty. AllocMem can fail; then there's no demo.
        move.l  ExecBase.w,a6
        move.l  #SCREENSIZE,d0
        move.l  #MEMF_CHIP!MEMF_CLEAR,d1
        jsr     _LVOAllocMem(a6)
        move.l  d0,Screen
        bne.s   .screen
        st      ExitNow
        rts
.screen:
        move.w  #$4200,CopperBplcon0+2  ; four bitplanes, colour on
        ; Its four bitplanes into the copper list: interleaved, each starts a line's worth (40 bytes) after the
        ; one before, and the modulos skip the other three at the end of each line.
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        add.l   #ROWBYTES,d0
        dbf     d1,.plane

        ; No sprites: the system's mouse pointer is one, and its sprite DMA stays on.
        move.l  #NoSprite,d0
        lea     CopperSprites,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
.pal:
        lea     DosName(pc),a1
        moveq   #33,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,DosBase
        bne.s   .dos
        st      ExitNow
.dos:
        rts
        endc

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the next piece of loading. The main task waits in dos.library for each piece (the disk may take a
; second or more), so nothing that must move smoothly can be done here: the copper bar moves in VBlank.

Frame:
        ifne    OS_FRIENDLY
        move.l  DosBase,a6
        move.w  LoadState,d0
        beq.s   .open
        subq.w  #1,d0
        beq.s   .read
        rts                             ; loaded (or failed): nothing more to do

.open:
        ; Open the file, from the current directory. Open takes its name and mode in d1 and d2.
        move.l  #PictureName,d1
        move.l  #MODE_OLDFILE,d2
        jsr     _LVOOpen(a6)
        move.l  d0,File
        beq     .failed
        ; The colours first: straight into the copper list's COLORxx moves.
        move.l  d0,d1
        move.l  #Palette,d2
        moveq   #COLOURS*2,d3
        jsr     _LVORead(a6)
        cmp.l   #COLOURS*2,d0
        bne     .failed
        lea     Palette,a0
        lea     CopperColours+2,a1
        moveq   #COLOURS-1,d0
.colour:
        move.w  (a0)+,(a1)
        addq.l  #4,a1
        dbf     d0,.colour
        move.w  Palette,CopperBarEnd+6  ; the copper bar ends in the picture's own background
        move.w  #1,LoadState
        rts

.read:
        ; A piece of the picture, straight into its place on the screen (dos.library can read into any memory).
        move.l  File,d1
        move.l  Screen,d2
        add.l   #PICTURETOP*LINEBYTES,d2
        add.l   Loaded,d2
        move.l  #PICTURESIZE,d3
        sub.l   Loaded,d3
        cmp.l   #CHUNKLINES*LINEBYTES,d3
        bls.s   .size
        move.l  #CHUNKLINES*LINEBYTES,d3
.size:
        jsr     _LVORead(a6)
        tst.l   d0                      ; -1: an error; 0: the end of the file, too soon
        ble.s   .failed
        add.l   d0,Loaded
        bsr     DrawProgress
        cmp.l   #PICTURESIZE,Loaded
        blo.s   .more
        bsr     CloseFile
        move.w  #2,LoadState
.more:
        rts

.failed:
        ; No file, or not all of it: the background goes red, and the demo carries on (left button to leave).
        bsr     CloseFile
        move.w  #$0a00,CopperColours+2
        move.w  #$0a00,CopperBarEnd+6
        move.w  #3,LoadState
        endc
        rts

CloseFile:
        move.l  File,d1
        beq.s   .none
        move.l  DosBase,a6
        jsr     _LVOClose(a6)
        clr.l   File
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The progress bar, drawn by the blitter: a block of 1s in bitplane 0, as many words wide as the share loaded.
; The blitter is shared with the system while it runs (trackdisk.device uses it to read floppy disks, and that may
; be going on right now), so BLIT_BEGIN asks for it - and waits, if the system's using it - and BLIT_END gives it
; back.

DrawProgress:
        move.l  Loaded,d0
        divu    #PICTURESIZE/(ROWBYTES/2),d0 ; 20 words across: one for each 1920 bytes
        tst.w   d0
        beq.s   .none
        cmp.w   ProgressWords,d0
        beq.s   .none
        move.w  d0,ProgressWords
        lea     CUSTOM,a5
        BLIT_BEGIN
        move.l  Screen,d1
        add.l   #BARTOP*LINEBYTES,d1
        move.l  d1,BLTDPTH(a5)
        moveq   #LINEBYTES/2,d1
        sub.w   d0,d1
        add.w   d1,d1
        move.w  d1,BLTDMOD(a5)          ; to the next line of bitplane 0
        move.l  #$01ff0000,BLTCON0(a5)  ; D only, minterm $ff: all 1s
        or.w    #BARLINES<<6,d0
        move.w  d0,BLTSIZE(a5)
        BLIT_END
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called first as the demo ends: the file closed if it's still open, and the memory and dos.library given back.

LoaderExit:
        bsr     CloseFile
        move.l  ExecBase.w,a6
        move.l  Screen,d0
        beq.s   .noscreen
        move.l  d0,a1
        move.l  #SCREENSIZE,d0          ; FreeMem needs the size it was given
        jsr     _LVOFreeMem(a6)
        clr.l   Screen
.noscreen:
        move.l  DosBase,d0
        beq.s   .nodos
        move.l  d0,a1
        jsr     _LVOCloseLibrary(a6)
.nodos:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt: the copper bar moves a line. This runs whatever the main task
; is doing, so it keeps going while a Read waits for the disk.

VBlank:
        move.w  CopperBarLine,d0
        add.w   CopperBarStep,d0
        cmp.w   #COPPERBARTOP,d0
        ble.s   .turn
        cmp.w   #COPPERBARBOTTOM-COPPERBARLINES,d0
        blt.s   .move
.turn:
        neg.w   CopperBarStep
.move:
        move.w  d0,CopperBarLine
        lea     CopperBar,a0
        moveq   #COPPERBARLINES,d1
.line:
        move.b  d0,(a0)
        addq.w  #1,d0
        addq.l  #8,a0
        dbf     d1,.line
        rts

DosName:        dc.b    "dos.library",0
PictureName:    dc.b    "ch15_picture.raw",0
        even

        section data,data

Screen:         dc.l    0
DosBase:        dc.l    0
File:           dc.l    0
Loaded:         dc.l    0               ; bytes of the picture read so far
LoadState:      dc.w    0               ; 0: to open, 1: reading, 2: loaded, 3: failed
ProgressWords:  dc.w    0
CopperBarLine:  dc.w    COPPERBARTOP+1
CopperBarStep:  dc.w    1
Palette:        ds.w    COLOURS

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
CopperBplcon0:
        dc.w    BPLCON0,$0200           ; no bitplanes until there's a screen (then four)
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,LINEBYTES-ROWBYTES
        dc.w    BPL2MOD,LINEBYTES-ROWBYTES
CopperColours:
        dc.w    COLOR00,$0102,COLOR01,$0fff ; until the picture's own colours come: the progress bar in white
        dc.w    COLOR02,0,COLOR03,0,COLOR04,0,COLOR05,0,COLOR06,0,COLOR07,0
        dc.w    COLOR08,0,COLOR09,0,COLOR10,0,COLOR11,0,COLOR12,0,COLOR13,0,COLOR14,0,COLOR15,0
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
CopperBar:
        ; It changes the background colour, so it shows wherever the picture has none of its own.
        dc.w    $4107,$fffe,COLOR00,$0213
        dc.w    $4207,$fffe,COLOR00,$0436
        dc.w    $4307,$fffe,COLOR00,$0759
        dc.w    $4407,$fffe,COLOR00,$0a8c
        dc.w    $4507,$fffe,COLOR00,$0a8c
        dc.w    $4607,$fffe,COLOR00,$0759
        dc.w    $4707,$fffe,COLOR00,$0436
        dc.w    $4807,$fffe,COLOR00,$0213
CopperBarEnd:
        dc.w    $4907,$fffe,COLOR00,$0102
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0
