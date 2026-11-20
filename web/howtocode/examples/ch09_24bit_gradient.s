; ch09_24bit_gradient.s - AGA only: a copper gradient in 24-bit colour beside the same gradient in 12-bit colour,
; so the difference shows. The left half is colour 0, set the old way (one write: 4 bits for each of red, green
; and blue); the right half is colour 1, set the AGA way (two writes, the high 4 bits and then the low 4 bits
; with BPLCON3's LOCT set). Every line has its own colour, and the gradient drifts up a line each frame. On a
; machine without AGA it shows nothing and ends at once. Left mouse button to leave.
;
; How To Code v8, chapter 9. CC0: use it for anything.
;
; $VER: ch09_24bit_gradient 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
ROWBYTES        equ     WIDTH/8
TOPLINE         equ     $2c             ; the display's first line
LINES           equ     256             ; how many lines the gradient covers (NTSC shows the top 200)
COLOURS         equ     256             ; the palette the gradient is made from (a power of two, to wrap with AND)
LINEBYTES       equ     7*4             ; each line's copper instructions: a wait and six more (see Init)

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; AGA or not? Without it the copper list ends after its first move (no bitplanes), and the demo ends.
        bsr     IsAGA
        bne.s   .aga
        move.l  #$fffffffe,CopperAGA
        st      ExitNow
        rts
.aga:
        ; The bitplanes: one row each, shown again on every line (the modulos are -40, so after each line the
        ; pointers go back to the row's start). Plane 1 is set on the right half, making it colour 1; plane 2 on
        ; a thin strip down the middle, making it colour 2 (black), to divide the halves.
        lea     Row1,a0
        moveq   #ROWBYTES/2/4-1,d0
.left:
        clr.l   (a0)+
        dbf     d0,.left
        moveq   #ROWBYTES/2/4-1,d0
.right:
        move.l  #$ffffffff,(a0)+
        dbf     d0,.right
        lea     Row2,a0
        move.l  #$00000003,ROWBYTES/2-4(a0) ; the two pixels either side of the middle
        move.l  #$c0000000,ROWBYTES/2(a0)
        and.l   #$3fffffff,Row1+ROWBYTES/2 ; and not colour 3
        move.l  #Row1,d0
        move.w  d0,CopperBitplanes+6
        swap    d0
        move.w  d0,CopperBitplanes+2
        move.l  #Row2,d0
        move.w  d0,CopperBitplanes+14
        swap    d0
        move.w  d0,CopperBitplanes+10

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
.pal:
        bsr     MakePalette
        bsr     MakeCopperLines
        rts

; ---------------------------------------------------------------------------------------------------------------
; Is this an AGA machine? Returns d0 = 1 (and the Z flag clear) if so, 0 if not.
;
; Lisa, AGA's video chip, answers a read of DENISEID ($DFF07C) with $F8 in the low byte. ECS's Denise answers $FC.
; The first Denise has no such register: a read gives whatever happens to be on the bus, which could be $F8 by
; chance. So read it many times: a chip that's really there gives the same answer every time.

IsAGA:
        move.w  DENISEID(a5),d0
        cmp.b   #$f8,d0
        bne.s   .no
        moveq   #32-1,d1
.again:
        move.w  DENISEID(a5),d2
        cmp.b   d0,d2
        bne.s   .no
        dbf     d1,.again
        moveq   #1,d0
        rts
.no:
        moveq   #0,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The palette: 256 colours of 24 bits, smooth steps between four key colours, 64 steps from each to the next
; (the last leads back to the first). Slow changes, so the 12-bit half shows its bands: a component that goes
; from $10 to $40 over 64 lines has 48 steps in 24 bits, and 3 in 12. Each colour is kept as the two 12-bit
; halves the colour registers take: the high nibbles of red, green and blue, then the low.

MakePalette:
        lea     KeyColours,a0
        lea     Palette,a1
        moveq   #4-1,d7                 ; four keys
.key:
        move.l  (a0)+,d0                ; from this colour ($00RRGGBB)
        move.l  (a0),d1                 ; to the next
        tst.w   d7
        bne.s   .notlast
        move.l  KeyColours,d1           ; the last key leads back to the first
.notlast:
        moveq   #0,d2                   ; the step, 0 to 63
.step:
        moveq   #0,d6                   ; the 24-bit colour being made
        moveq   #16,d5                  ; red (bits 16-23), then green (8-15), then blue (0-7)
.component:
        move.l  d0,d3
        lsr.l   d5,d3
        and.w   #$ff,d3                 ; from
        move.l  d1,d4
        lsr.l   d5,d4
        and.w   #$ff,d4                 ; to
        sub.w   d3,d4
        muls    d2,d4
        asr.l   #6,d4                   ; (to - from) * step / 64
        add.w   d3,d4
        and.l   #$ff,d4
        lsl.l   d5,d4
        or.l    d4,d6
        subq.w  #8,d5
        bpl.s   .component
        ; Split it: $00RrGgBb becomes $0RGB (the high nibbles) and $0rgb (the low).
        move.l  d6,d3
        lsr.l   #4,d3
        move.w  d3,d4
        and.w   #$000f,d4               ; blue's high nibble
        lsr.l   #4,d3
        move.w  d3,d5
        and.w   #$00f0,d5               ; green's
        or.w    d5,d4
        lsr.l   #4,d3
        and.w   #$0f00,d3               ; red's
        or.w    d3,d4
        move.w  d4,(a1)+
        move.w  d6,d4
        and.w   #$000f,d4               ; blue's low nibble
        move.l  d6,d3
        lsr.l   #4,d3
        move.w  d3,d5
        and.w   #$00f0,d5               ; green's
        or.w    d5,d4
        lsr.l   #4,d3
        and.w   #$0f00,d3               ; red's
        or.w    d3,d4
        move.w  d4,(a1)+
        addq.w  #1,d2
        cmp.w   #64,d2
        blo.s   .step
        dbf     d7,.key
        rts

; ---------------------------------------------------------------------------------------------------------------
; The copper list's lines. For each line of the display:
;
;       wait for the line's start           (left of the display: the changes are made out of sight)
;       COLOR00 = the colour, high halves   the 12-bit half: one write, and AGA copies the high nibbles to the low
;       COLOR01 = the colour, high halves   the 24-bit half: first the high nibbles, which also sets the low...
;       BPLCON3 = LOCT set                  (the rest of BPLCON3 as at the top of the list)
;       COLOR01 = the colour, low halves    ...then the low nibbles, with LOCT set
;       BPLCON3 = LOCT clear
;       a NOOP; after line 255, a wait for the end of that line instead, to cross into lines 256 and on
;
; The colours themselves are filled in by Frame.

MakeCopperLines:
        lea     CopperLines,a0
        move.w  #TOPLINE,d0
        move.w  #LINES-1,d7
.line:
        move.w  d0,d1
        lsl.w   #8,d1                   ; the line in the top byte (only its low 8 bits: line 256 is 0 again)
        or.w    #$07,d1
        move.w  d1,(a0)+
        move.w  #$fffe,(a0)+
        move.w  #COLOR00,(a0)+
        clr.w   (a0)+
        move.w  #COLOR00+2,(a0)+
        clr.w   (a0)+
        move.w  #BPLCON3,(a0)+
        move.w  #$0e20,(a0)+            ; LOCT (bit 9) set: colour writes go to the low nibbles
        move.w  #COLOR00+2,(a0)+
        clr.w   (a0)+
        move.w  #BPLCON3,(a0)+
        move.w  #$0c20,(a0)+            ; LOCT clear again
        cmp.w   #255,d0
        bne.s   .noop
        move.l  #$ffdffffe,(a0)+        ; line 255's end: the next waits are for 256 and on
        bra.s   .next
.noop:
        move.l  #$01fe0000,(a0)+        ; NOOP: a move to a register that isn't there, so every line is the
.next:                                  ; same length
        addq.w  #1,d0
        dbf     d7,.line
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: every line's colours written again, a line further on in the palette. This is near the top of
; the frame, before the copper reaches the first of them.

Frame:
        tst.b   ExitNow                 ; no AGA: nothing to do
        bne.s   .done
        move.l  Frames,d0
        lea     CopperLines,a0
        lea     Palette,a1
        moveq   #0,d1                   ; the line
        move.w  #LINES-1,d7
.line:
        move.w  d1,d2
        add.w   d0,d2
        and.w   #COLOURS-1,d2
        lsl.w   #2,d2
        move.w  0(a1,d2.w),d3           ; the high halves
        move.w  d3,6(a0)                ; COLOR00
        move.w  d3,10(a0)               ; COLOR01, high
        move.w  2(a1,d2.w),18(a0)       ; COLOR01, low
        lea     LINEBYTES(a0),a0
        addq.w  #1,d1
        dbf     d7,.line
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do here.

VBlank:
        rts

        section data,data

; The key colours, 24 bits each: night, violet, rose, gold.
KeyColours:
        dc.l    $00081030,$003a1460,$00b03060,$00f0a040

        section bss,bss

Palette:        ds.w    COLOURS*2       ; each colour's high half, then its low half

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the bitplanes' two rows, in chip RAM.

        section copper,data_c

Copper:
        dc.w    BPLCON0,$0200           ; no bitplanes until the rest is ready (without AGA, all there is)
CopperAGA:
        dc.w    FMODE,$0000             ; plain 16-bit fetches: two bitplanes need no more
        dc.w    BPLCON3,$0c20           ; colour bank 0, LOCT clear, sprites at their usual resolution, and
                                        ; BRDRBLNK: the border black, whatever colour 0 is
        dc.w    BPLCON4,$0011           ; no XOR on the colours; sprites take colours 16-31, as they always have
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,-ROWBYTES       ; back to the row's start after every line: the same row on every line
        dc.w    BPL2MOD,-ROWBYTES
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0000
        dc.w    COLOR02,$0000           ; the dividing line: black
        dc.w    COLOR03,$0000
        dc.w    BPLCON0,$2201           ; two bitplanes, colour on, and ECSENA (bit 0), for BRDRBLNK
CopperLines:
        ds.b    LINES*LINEBYTES         ; made by MakeCopperLines
        dc.w    COLOR00,$0000           ; below the gradient, black (on NTSC this comes too late to be seen)
        dc.w    COLOR01,$0000
        dc.w    $ffff,$fffe

        cnop    0,4
Row1:   ds.b    ROWBYTES
Row2:   ds.b    ROWBYTES
