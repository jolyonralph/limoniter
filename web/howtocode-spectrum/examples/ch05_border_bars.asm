; ch05_border_bars.asm: raster bars in the border, timed to the T-state. From the interrupt, the program waits
; until line 40 of the frame, then changes the border once a line for 240 lines, in a loop that takes exactly as
; long as a scan line: 224 T-states on a 48K, 228 on a 128K (the loop has a slot of two bytes that Init fills
; with two NOPs or a JR to the next instruction, 8 or 12 T-states). Each change is made while the beam is in the
; horizontal retrace, where it can't be seen, so every line is one colour. Chapter 5, The screen. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

LINES           equ     240
; The waits before the first line, in 26 T-states, worked out in chapter 5 from when Interrupt starts (about 174
; T-states into the frame): the first change lands in the retrace of line 40 (48K) or 39 (128K).
DELAY48         equ     341
DELAY128        equ     338

Init:
        xor     a
        call    ClearScreen
        ld      bc,$0B06
        ld      hl,Text
        call    PrintAt
        ld      a,(Is128)               ; the slot: JR $+2 on a 128K, two NOPs on a 48K
        or      a
        ld      hl,0
        jr      z,.slot
        ld      hl,$0018                ; $18 $00: JR to the next instruction
.slot:
        ld      (Slot),hl
        ret

Frame:
        ret

Interrupt:
        ld      a,(Is128)               ; 13
        or      a                       ; 4
        jr      nz,.wait128             ; 7 or 12
        ld      bc,DELAY48              ; 10
        jr      .wait                   ; 12
.wait128:
        ld      bc,DELAY128             ; 10
        nop                             ; 4
.wait:
        dec     bc                      ; 6
        ld      a,b                     ; 4
        or      c                       ; 4
        jr      nz,.wait                ; 12 (7 the last time)
        ld      a,(Frames)              ; 13: the bars move up a line each frame
        ld      l,a                     ; 4
        ld      h,Bars/256              ; 7
        ld      bc,$00FE                ; 10: OUT (C) to port $FE, with B 0 as DJNZ leaves it
        ld      e,LINES                 ; 7
BarLine:
        ld      a,(hl)                  ; 7
        out     (c),a                   ; 12: the border changes near the end of this
        inc     hl                      ; 6
        ld      b,13                    ; 7
.delay:
        djnz    .delay                  ; 13 x 12 + 8 = 164
        nop                             ; 4
Slot:
        nop                             ; 8 on a 48K (two NOPs), 12 on a 128K (JR $+2)
        nop
        dec     e                       ; 4
        jr      nz,BarLine             ; 12: 224 or 228 in all
        xor     a                       ; black for the rest of the frame
        out     ($FE),a
        ret

Text:           db      "224 T-states a line",0

        align   256
; a colour for each line: four bars in 256 lines, the table twice so 240 lines can start anywhere in the first 256
Bars:
        db      1,1,1,5,5,5,7,7,7,7,7,7,5,5,5,1
        db      1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      2,2,2,6,6,6,7,7,7,7,7,7,6,6,6,2
        db      2,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      3,3,3,7,7,7,7,7,7,3,3,3,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      4,4,4,6,6,6,7,7,7,7,7,7,6,6,6,4
        db      4,4,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      1,1,1,5,5,5,7,7,7,7,7,7,5,5,5,1
        db      1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      2,2,2,6,6,6,7,7,7,7,7,7,6,6,6,2
        db      2,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      3,3,3,7,7,7,7,7,7,3,3,3,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      4,4,4,6,6,6,7,7,7,7,7,7,6,6,6,4
        db      4,4,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        db      0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
