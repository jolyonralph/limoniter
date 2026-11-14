; The smallest useful Amiga copper program: it takes the machine over from the system, shows a copper list
; changing the background colour line by line (no bitplanes at all), waits for the left mouse button, and gives
; everything back. A starting point for trying out copper lists.
;
; Devpac/vasm syntax. To make a program the Amiga runs:
;   vasmm68k_mot -Fhunkexe -kick1hunks -nosym -o copper copper.s

ExecBase        equ     4
_LVOForbid      equ     -132
_LVOPermit      equ     -138
_LVOCloseLibrary equ    -414
_LVOOpenLibrary equ     -552
_LVOLoadView    equ     -222            ; graphics.library
_LVOWaitBlit    equ     -228
_LVOWaitTOF     equ     -270
_LVOOwnBlitter  equ     -456
_LVODisownBlitter equ   -462
gb_ActiView     equ     34
gb_copinit      equ     38

CUSTOM          equ     $dff000         ; the custom chips, kept in a5
DMACONR         equ     $002
INTENAR         equ     $01c
INTREQR         equ     $01e
COP1LC          equ     $080
COPJMP1         equ     $088
DMACON          equ     $096
INTENA          equ     $09a
INTREQ          equ     $09c
BPLCON0         equ     $100
SPR0PTH         equ     $120
COLOR00         equ     $180
FMODE           equ     $1fc
CIAAPRA         equ     $bfe001         ; bit 6 is 0 while the left mouse button is down

        section code,code

start:
        movem.l d1-d7/a0-a6,-(sp)
        move.l  ExecBase.w,a6
        lea     gfxname(pc),a1
        moveq   #0,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,gfxbase
        beq     nogfx

        ; Take the blitter and empty the display, then stop multitasking.
        move.l  d0,a6
        move.l  gb_ActiView(a6),oldview
        jsr     _LVOOwnBlitter(a6)
        jsr     _LVOWaitBlit(a6)
        sub.l   a1,a1
        jsr     _LVOLoadView(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVOWaitTOF(a6)
        move.l  ExecBase.w,a6
        jsr     _LVOForbid(a6)

        ; Save DMA and interrupts (bit 15 set, to switch them back on), then switch them all off.
        lea     CUSTOM,a5
        move.w  DMACONR(a5),d0
        or.w    #$8000,d0
        move.w  d0,olddmacon
        move.w  INTENAR(a5),d0
        or.w    #$8000,d0
        move.w  d0,oldintena
        move.w  #$7fff,INTENA(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,DMACON(a5)

        ; Every sprite at an empty one, so none shows.
        move.l  #nosprite,d0
        lea     spriteptrs,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite

        ; Our copper list from the top of a frame (VERTB in INTREQ is set at the start of each one).
.wait:
        btst    #5,INTREQR+1(a5)
        beq.s   .wait
        move.l  #copper,COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        move.w  #$83a0,DMACON(a5)       ; SET, DMAEN, COPEN, SPREN

        ; Nothing to do each frame: the copper does it all.
mouse:
        btst    #6,CIAAPRA
        bne.s   mouse

        ; Put everything back.
        move.w  #$7fff,DMACON(a5)
        move.w  #$7fff,INTENA(a5)
        move.w  olddmacon,DMACON(a5)
        move.w  oldintena,INTENA(a5)
        move.l  gfxbase,a6
        move.l  gb_copinit(a6),COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        move.l  oldview,a1
        jsr     _LVOLoadView(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVODisownBlitter(a6)
        move.l  ExecBase.w,a6
        jsr     _LVOPermit(a6)
        move.l  gfxbase,a1
        jsr     _LVOCloseLibrary(a6)
nogfx:
        movem.l (sp)+,d1-d7/a0-a6
        moveq   #0,d0
        rts

gfxname:
        dc.b    "graphics.library",0
        even

        section vars,data

gfxbase:        dc.l    0
oldview:        dc.l    0
olddmacon:      dc.w    0
oldintena:      dc.w    0

        section copper,data_c

copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches (older chips ignore it)
        dc.w    BPLCON0,$0200           ; no bitplanes, colour on: the background is all there is
spriteptrs:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        ; Each line: wait for it (line, then $07 for the left edge; $fffe is the mask), then a colour.
        dc.w    COLOR00,$0000
        dc.w    $4007,$fffe,COLOR00,$0005
        dc.w    $5007,$fffe,COLOR00,$000a
        dc.w    $6007,$fffe,COLOR00,$000f
        dc.w    $7007,$fffe,COLOR00,$005f
        dc.w    $8007,$fffe,COLOR00,$00af
        dc.w    $9007,$fffe,COLOR00,$00ff
        dc.w    $a007,$fffe,COLOR00,$0fff
        dc.w    $b007,$fffe,COLOR00,$0ff0
        dc.w    $c007,$fffe,COLOR00,$0fa0
        dc.w    $d007,$fffe,COLOR00,$0f50
        dc.w    $e007,$fffe,COLOR00,$0f00
        dc.w    $f007,$fffe,COLOR00,$0000
        ; Lines past 255 need the wait for line 255's end first, as the wait's line number is only 8 bits.
        dc.w    $ffdf,$fffe
        dc.w    $1007,$fffe,COLOR00,$0500
        dc.w    $2007,$fffe,COLOR00,$0000
        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

nosprite:
        dc.l    0,0
