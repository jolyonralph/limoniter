; A starting point for an Amiga demo, for any Amiga from the A500 on (OCS, ECS or AGA, Kickstart 1.3 or later).
;
; It takes the machine over from the system politely, so it can be given back afterwards: it opens
; graphics.library, takes the blitter, empties the display with LoadView(0), stops multitasking with Forbid, and
; saves the custom chips' DMA, interrupt and audio settings. Then its own copper list shows one bitplane (a
; chequerboard, 320 by 256) with a raster bar bouncing over it, until the left mouse button is pressed, when
; everything is put back as it was.
;
; Devpac/vasm syntax. To make a program the Amiga runs:
;   vasmm68k_mot -Fhunkexe -kick1hunks -nosym -o demo demo.s
; Everything it needs is defined here, so no include files are needed.

; ---------------------------------------------------------------------------------------------------------------
; Library calls, as offsets from the library's base in a6

ExecBase        equ     4               ; where exec.library's base is kept
_LVOForbid      equ     -132
_LVOPermit      equ     -138
_LVOCloseLibrary equ    -414
_LVOOpenLibrary equ     -552

_LVOLoadView    equ     -222            ; graphics.library
_LVOWaitBlit    equ     -228
_LVOWaitTOF     equ     -270
_LVOOwnBlitter  equ     -456
_LVODisownBlitter equ   -462

gb_ActiView     equ     34              ; in GfxBase: the view on show
gb_copinit      equ     38              ; and the system's own copper list

; ---------------------------------------------------------------------------------------------------------------
; The custom chips, as offsets from $DFF000 (kept in a5 throughout)

CUSTOM          equ     $dff000
DMACONR         equ     $002
VPOSR           equ     $004
ADKCONR         equ     $010
INTENAR         equ     $01c
INTREQR         equ     $01e
COP1LC          equ     $080
COPJMP1         equ     $088
DIWSTRT         equ     $08e
DIWSTOP         equ     $090
DDFSTRT         equ     $092
DDFSTOP         equ     $094
DMACON          equ     $096
INTENA          equ     $09a
INTREQ          equ     $09c
ADKCON          equ     $09e
BPL1PTH         equ     $0e0
BPL1PTL         equ     $0e2
BPLCON0         equ     $100
BPLCON1         equ     $102
BPLCON2         equ     $104
BPLCON3         equ     $106
BPL1MOD         equ     $108
BPL2MOD         equ     $10a
SPR0PTH         equ     $120
COLOR00         equ     $180
COLOR01         equ     $182
FMODE           equ     $1fc

CIAAPRA         equ     $bfe001         ; bit 6 is 0 while the left mouse button is down

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
BARLINES        equ     8               ; how tall the raster bar is
BARTOP          equ     $40             ; how high and low it bounces (screen lines, $2c is the top)
BARBOTTOM       equ     $f0

; ---------------------------------------------------------------------------------------------------------------

        section code,code

start:
        movem.l d1-d7/a0-a6,-(sp)

        ; graphics.library, any version
        move.l  ExecBase.w,a6
        lea     gfxname(pc),a1
        moveq   #0,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,gfxbase
        beq     nogfx

        ; Note the system's view, take the blitter, and show nothing: LoadView(0) puts the display back to
        ; plain settings (no AGA modes or interlace left on), and waiting two frames lets it take effect.
        move.l  d0,a6
        move.l  gb_ActiView(a6),oldview
        jsr     _LVOOwnBlitter(a6)
        jsr     _LVOWaitBlit(a6)
        sub.l   a1,a1
        jsr     _LVOLoadView(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVOWaitTOF(a6)

        ; No other task runs from here on.
        move.l  ExecBase.w,a6
        jsr     _LVOForbid(a6)

        ; Save the chips' settings. Reading gives the bits that are on; bit 15 set makes writing them back
        ; switch the same bits on again.
        lea     CUSTOM,a5
        move.w  DMACONR(a5),d0
        or.w    #$8000,d0
        move.w  d0,olddmacon
        move.w  INTENAR(a5),d0
        or.w    #$8000,d0
        move.w  d0,oldintena
        move.w  INTREQR(a5),d0
        or.w    #$8000,d0
        move.w  d0,oldintreq
        move.w  ADKCONR(a5),d0
        or.w    #$8000,d0
        move.w  d0,oldadkcon

        ; Every interrupt and every kind of DMA off: the machine is ours.
        move.w  #$7fff,INTENA(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,DMACON(a5)

        bsr     setup

        ; Start the copper list at the top of a frame, then DMA for the bitplane, the copper, the blitter and
        ; sprites (the sprites point at an empty one, so none shows).
        bsr     waitvbl
        move.l  #copper,COP1LC(a5)
        move.w  d0,COPJMP1(a5)          ; writing anything here starts the copper list
        move.w  #$83e0,DMACON(a5)       ; SET, DMAEN, BPLEN, COPEN, BLTEN, SPREN

; ---------------------------------------------------------------------------------------------------------------
; The main loop: once a frame, until the left mouse button is pressed.
;
; Waiting for the frame: the chips set bit 5 of INTREQ (VERTB) at the start of every frame whether or not the
; interrupt is enabled, so the loop waits for it and clears it. That's simpler than a level 3 interrupt, and
; as robust: no vector to put in place (on a 68010 or later the vectors may not be at address 0, which an
; interrupt would have to allow for) and nothing to undo. A frame's work that overruns just misses a frame.

mainloop:
        bsr     waitvbl
        bsr     frame
        btst    #6,CIAAPRA
        bne.s   mainloop

; ---------------------------------------------------------------------------------------------------------------
; Put everything back as it was.

        bsr.s   waitvbl
        move.l  gfxbase,a6
        jsr     _LVOWaitBlit(a6)        ; in case a blit is still going

        move.w  #$7fff,DMACON(a5)
        move.w  #$7fff,INTENA(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,ADKCON(a5)
        move.w  oldadkcon,ADKCON(a5)
        move.w  oldintreq,INTREQ(a5)
        move.w  olddmacon,DMACON(a5)
        move.w  oldintena,INTENA(a5)

        ; The system's copper list, then its view.
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
        moveq   #0,d0                   ; all went well
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits for the next frame to begin: see the main loop.

waitvbl:
        btst    #5,INTREQR+1(a5)
        beq.s   waitvbl
        move.w  #$0020,INTREQ(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Gets the screen and copper list ready before the display starts.

setup:
        ; The bitplane's address into the copper list, high word then low word.
        move.l  #screen,d0
        move.w  d0,bplptr+6
        swap    d0
        move.w  d0,bplptr+2

        ; All eight sprites at an empty sprite.
        move.l  #nosprite,d0
        lea     spriteptrs,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)                ; high word
        swap    d0
        move.w  d0,6(a0)                ; low word
        addq.l  #8,a0
        dbf     d1,.sprite

        ; A chequerboard of 16 by 16 squares: rows of $FFFF0000, turned over every 16 rows.
        lea     screen,a0
        move.w  #HEIGHT-1,d7
.row:
        move.l  #$ffff0000,d1
        btst    #4,d7
        beq.s   .fill
        not.l   d1
.fill:
        moveq   #ROWBYTES/4-1,d6
.long:
        move.l  d1,(a0)+
        dbf     d6,.long
        dbf     d7,.row
        rts

; ---------------------------------------------------------------------------------------------------------------
; A frame's work: moves the raster bar a line up or down, by rewriting the line numbers in its copper waits.

frame:
        move.w  bary,d0
        add.w   bardir,d0
        cmp.w   #BARTOP,d0
        ble.s   .turn
        cmp.w   #BARBOTTOM-BARLINES,d0
        blt.s   .move
.turn:
        neg.w   bardir
.move:
        move.w  d0,bary

        ; Each line of the bar is a wait (line, $07) then a colour; one more wait puts the background back.
        lea     bar,a0
        moveq   #BARLINES,d1            ; BARLINES+1 waits, as dbf counts down to -1
.line:
        move.b  d0,(a0)
        addq.w  #1,d0
        addq.l  #8,a0
        dbf     d1,.line
        rts

gfxname:
        dc.b    "graphics.library",0
        even

; ---------------------------------------------------------------------------------------------------------------
; Things that change as it runs.

        section vars,data

gfxbase:        dc.l    0
oldview:        dc.l    0
olddmacon:      dc.w    0
oldintena:      dc.w    0
oldintreq:      dc.w    0
oldadkcon:      dc.w    0
bary:           dc.w    BARTOP+1
bardir:         dc.w    1

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip memory, where the chips can read them.

        section copper,data_c

copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these two)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; a 320 by 256 window, the usual PAL screen
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0368
bplptr:
        dc.w    BPL1PTH,0,BPL1PTL,0     ; filled in by setup
spriteptrs:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
bar:
        ; Wait for line $41, from the left edge ($07 and the mask $fffe), then set the background colour.
        dc.w    $4107,$fffe,COLOR00,$0102
        dc.w    $4207,$fffe,COLOR00,$0315
        dc.w    $4307,$fffe,COLOR00,$062a
        dc.w    $4407,$fffe,COLOR00,$0c4f
        dc.w    $4507,$fffe,COLOR00,$0c4f
        dc.w    $4607,$fffe,COLOR00,$062a
        dc.w    $4707,$fffe,COLOR00,$0315
        dc.w    $4807,$fffe,COLOR00,$0102
        dc.w    $4907,$fffe,COLOR00,$0000
        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

nosprite:
        dc.l    0,0

; ---------------------------------------------------------------------------------------------------------------
; The screen, in chip memory too.

        section screen,bss_c

screen:
        ds.b    ROWBYTES*HEIGHT
