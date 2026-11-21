; ch17_cd32_pad.s - reading the CD32's pad in the second port (where a joystick goes): its seven buttons, read one
; after another from the pad's shift register, and its four directions. Each lights its box: the directions on the
; left, red, blue, green and yellow on the right, the two shoulder buttons along the top, play in the middle. The
; bar along the bottom is green when a pad answers, and dark red when it doesn't (a plain joystick, or nothing).
; Left mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything.
;
; The pins it changes (pins 5 and 9 in POTGO) are asked for from potgo.resource, both ways, as chapter 11 says.
; The fire pin is made an output while the buttons are clocked out, and an input again straight after.
;
; $VER: ch17_cd32_pad 8.0 (21.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

PLANESIZE       equ     40*256
OUTRY           equ     $8000           ; POTGO: port 1's pin 9 an output...
DATRY           equ     $4000           ; ...and its level (in POTGOR: the pin's level, read as a button)
OUTRX           equ     $2000           ; port 1's pin 5 an output...
DATRX           equ     $1000           ; ...and its level
PORT1BITS       equ     OUTRY!DATRY!OUTRX!DATRX
_LVOAllocPotBits equ    -6
_LVOFreePotBits equ     -12
_LVOWritePotgo  equ     -18
CIAB_FIRE1      equ     7               ; CIA-A's port A: port 1's fire pin (pin 6), the pad's clock

        section code,code

Init:
        move.l  #PadExit,ExitHook
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
        move.l  #Screen,d0
        lea     CopperBitplanes,a0
        moveq   #4-1,d1
.plane:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        add.l   #PLANESIZE,d0
        addq.l  #8,a0
        dbf     d1,.plane

        ; The screen starts empty, then a box for each control, in its own colour (1 to 12).
        lea     Screen,a0
        move.w  #PLANESIZE*4/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear
        lea     Boxes(pc),a2
        moveq   #12-1,d7
.box:
        movem.w (a2)+,d0-d4
        bsr     FillBox
        dbf     d7,.box

        ; The pins: pins 5 and 9 of port 1, from potgo.resource.
        move.l  ExecBase.w,a6
        lea     PotgoName(pc),a1
        jsr     _LVOOpenResource(a6)
        move.l  d0,PotgoBase
        beq.s   .nopotgo
        move.l  d0,a6
        move.l  #PORT1BITS,d0
        jsr     _LVOAllocPotBits(a6)
        move.w  d0,PotBits              ; what we were given: all four, or we read no buttons
        cmp.w   #PORT1BITS,d0
        beq.s   .ours
        jsr     _LVOFreePotBits(a6)
        clr.w   PotBits
        bra.s   .nopotgo
.ours:
        bsr     Idle
.nopotgo:
        rts

; As the program ends: the pins back to inputs, and given back.
PadExit:
        tst.w   PotBits
        beq.s   .none
        move.l  PotgoBase,a6
        moveq   #0,d0
        move.l  #PORT1BITS,d1
        jsr     _LVOWritePotgo(a6)
        moveq   #0,d0
        move.w  PotBits,d0
        jsr     _LVOFreePotBits(a6)
.none:
        rts

; Pin 5 high (the pad's ordinary mode: loading its buttons, and red on the fire pin, blue on pin 9, as a two button
; joystick) and pin 9 pulled up, so it reads as a button.
Idle:
        move.l  PotgoBase,a6
        move.l  #PORT1BITS,d0
        move.l  #PORT1BITS,d1
        jmp     _LVOWritePotgo(a6)

; A box: d0 x and d1 y in pixels (x a multiple of 8), d2 width (a multiple of 8), d3 height, d4 colour.
FillBox:
        lea     Screen,a0
        mulu    #40,d1
        add.l   d1,a0
        lsr.w   #3,d0
        add.w   d0,a0
        lsr.w   #3,d2
        subq.w  #1,d2
        subq.w  #1,d3
        moveq   #0,d5                   ; each bitplane: set where the colour has its bit
.plane:
        btst    d5,d4
        beq.s   .skip
        move.l  a0,a1
        move.w  d3,d6
.line:
        move.l  a1,a3
        move.w  d2,d1
.byte:
        st      (a3)+
        dbf     d1,.byte
        add.w   #40,a1
        dbf     d6,.line
.skip:
        add.l   #PLANESIZE,a0
        addq.w  #1,d5
        cmp.w   #4,d5
        blo.s   .plane
        rts

; The boxes: x, y, width, height, colour. In the order of the bits ReadPad returns, so box n lights for bit n.
Boxes:
        dc.w    80,94,24,24,1           ; right
        dc.w    32,94,24,24,2           ; left
        dc.w    56,118,24,24,3          ; down
        dc.w    56,70,24,24,4           ; up
        dc.w    144,100,32,14,5         ; play
        dc.w    40,36,64,12,6           ; reverse: the left shoulder
        dc.w    216,36,64,12,7          ; forward: the right shoulder
        dc.w    208,94,24,24,8          ; green: the left of the four
        dc.w    232,70,24,24,9          ; yellow: the top
        dc.w    232,118,24,24,10        ; red: the bottom
        dc.w    256,94,24,24,11         ; blue: the right
        dc.w    40,180,240,10,12        ; a pad answered

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: read the pad, and light the boxes.

Frame:
        bsr     ReadPad
        lea     Lit(pc),a0
        lea     Dim(pc),a1
        lea     CopperColours+2,a2
        moveq   #12-1,d7
        moveq   #0,d1
.colour:
        move.w  (a1)+,d2
        btst    d1,d0
        beq.s   .dim
        move.w  (a0),d2
.dim:
        addq.l  #2,a0
        move.w  d2,(a2)
        addq.l  #4,a2
        addq.w  #1,d1
        dbf     d7,.colour
        rts

; Each box's colour lit, and not.
Lit:    dc.w    $0fff,$0fff,$0fff,$0fff,$0ddd,$0ccc,$0ccc,$02e4,$0fe2,$0f33,$047f,$02c2
Dim:    dc.w    $0333,$0333,$0333,$0333,$0333,$0333,$0333,$0131,$0331,$0311,$0114,$0400

; The pad in port 1. Returns d0: bit 0 right, 1 left, 2 down, 3 up (from JOY1DAT, as a joystick's), then 4 play,
; 5 reverse, 6 forward, 7 green, 8 yellow, 9 red, 10 blue, and 11 if a CD32 pad answered.
;
; The pad holds its seven buttons in a shift register. Pin 5 low puts it in shift mode, with the first button
; (blue) on pin 9; each rising edge on pin 6 (the fire pin, which the Amiga drives for this) moves the next one
; onto pin 9. Two more come after the seven, a 1 and then a 0: a pad's way of saying it's there. Each bit is 0
; for a button held, as the Amiga's buttons are.
ReadPad:
        move.w  JOY1DAT(a5),d0
        move.w  d0,d1
        lsr.w   #1,d1
        eor.w   d0,d1                   ; bit 8 up, bit 0 down (chapter 11)
        moveq   #0,d2
        btst    #1,d0
        beq.s   .notright
        bset    #0,d2
.notright:
        btst    #9,d0
        beq.s   .notleft
        bset    #1,d2
.notleft:
        btst    #0,d1
        beq.s   .notdown
        bset    #2,d2
.notdown:
        btst    #8,d1
        beq.s   .notup
        bset    #3,d2
.notup:
        move.w  d2,Directions
        tst.w   PotBits
        beq     .nopins

        ; The fire pin an output, low; then pin 5 low, for shift mode, with pin 9 an input.
        bclr    #CIAB_FIRE1,CIAA+CIAPRA
        bset    #CIAB_FIRE1,CIAA+CIADDRA
        move.l  PotgoBase,a6
        move.l  #OUTRX,d0               ; pin 5: an output, low; pin 9: an input
        move.l  #PORT1BITS,d1
        jsr     _LVOWritePotgo(a6)

        moveq   #0,d2
        moveq   #9-1,d3
.bit:
        ; A little time for the pad's output to settle. Each read of a CIA register waits for the CIAs' own slow
        ; clock, so these take the same time on any processor, unlike a loop. (The WHDLoad package's pad reader
        ; waits the same way: "timing fix by wepl".)
        tst.b   CIAA+CIAPRA
        tst.b   CIAA+CIAPRA
        tst.b   CIAA+CIAPRA
        tst.b   CIAA+CIAPRA
        tst.b   CIAA+CIAPRA
        tst.b   CIAA+CIAPRA
        add.w   d2,d2
        move.w  POTGOR(a5),d0
        btst    #14,d0                  ; pin 9
        bne.s   .up
        addq.w  #1,d2                   ; low: held
.up:
        bset    #CIAB_FIRE1,CIAA+CIAPRA ; the clock up, and down: the next bit onto pin 9
        bclr    #CIAB_FIRE1,CIAA+CIAPRA
        dbf     d3,.bit

        ; The fire pin an input again, and the pad back in its ordinary mode.
        bclr    #CIAB_FIRE1,CIAA+CIADDRA
        bsr     Idle                    ; pins 5 and 9 outputs, high

        ; d2: blue in bit 8 ... play in bit 2, then the two that say a pad's there: 1, 0 as read, so 0, 1 here.
        move.w  d2,d0
        and.w   #3,d0
        cmp.w   #1,d0
        bne.s   .nopad
        lsr.w   #2,d2                   ; play in bit 0 ... blue in bit 6
        lsl.w   #4,d2                   ; play in bit 4 ... blue in bit 10
        bset    #11,d2
        or.w    Directions,d2
        move.w  d2,d0
        rts
.nopad:
.nopins:
        move.w  Directions,d0
        rts

VBlank:
        rts

PotgoName:      dc.b    "potgo.resource",0
        even

        section data,data

PotgoBase:      dc.l    0
PotBits:        dc.w    0
Directions:     dc.w    0

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$4200
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0112
CopperColours:
        dc.w    COLOR01,0,COLOR02,0,COLOR03,0,COLOR04,0,COLOR05,0,COLOR06,0
        dc.w    COLOR07,0,COLOR08,0,COLOR09,0,COLOR10,0,COLOR11,0,COLOR12,0
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
        dc.w    $ffff,$fffe

NoSprite:       dc.l    0

        section screen,bss_c

Screen:         ds.b    PLANESIZE*4
