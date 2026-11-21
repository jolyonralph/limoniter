; ch07_beeper.asm: two voices from the beeper. The beeper is one bit (bit 4 of port $FE), on or off: no volume,
; no second voice. Each voice is a square wave, made by adding a step to a 16-bit counter over and over and
; taking its top bit; the two are mixed by giving the beeper each in turn, many thousand times a second, faster
; than the speaker can follow (pulse interleaving). A tune of 32 rows plays round, a new pair of notes every eight
; frames. Each frame plays 530 samples, 125 T-states each (28,000 a second), and stops before the next interrupt,
; so the startup's main loop goes on as usual. Chapter 7, Sound. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

SAMPLES         equ     530             ; 530 x 125 = 66,250 T-states: a frame, less time for the interrupt
ROW_FRAMES      equ     8

Init:
        ld      a,%00111000
        call    ClearScreen
        ld      bc,$0A04
        ld      hl,Text
        call    PrintAt
        ld      hl,Tune
        ld      (Row),hl
        ret

Frame:
        ; A new pair of notes every eight frames: their steps from the tune.
        ld      a,(Frames)
        and     ROW_FRAMES-1
        jr      nz,.play
        ld      hl,(Row)
        ld      e,(hl)
        inc     hl
        ld      d,(hl)
        inc     hl
        ld      (Step1),de
        ld      e,(hl)
        inc     hl
        ld      d,(hl)
        inc     hl
        ld      (Step2),de
        ld      de,TuneEnd
        or      a
        push    hl
        sbc     hl,de
        pop     hl
        jr      nz,.next
        ld      hl,Tune
.next:
        ld      (Row),hl
.play:
        ld      hl,(Phase1)
        ld      de,(Step1)
        exx
        ld      hl,(Phase2)
        ld      de,(Step2)
        exx
        ld      b,SAMPLES&255           ; 18 the first time round B, then 256 (B from 0) twice more
        ld      c,SAMPLES/256+1         ; 3 times round in all (SAMPLES mustn't be a multiple of 256)
.sample:
        add     hl,de                   ; 11: voice 1 moves on
        ld      a,h                     ; 4
        and     %10000000               ; 7: its top bit
        rrca                            ; 4: to bit 4, the beeper
        rrca                            ; 4
        rrca                            ; 4
        or      %00001000               ; 7: MIC off (bit 3 set), the border black
        out     ($FE),a                 ; 11: voice 1 is heard for the next 60 T-states
        exx                             ; 4
        add     hl,de                   ; 11: voice 2
        ld      a,h                     ; 4
        exx                             ; 4
        and     %10000000               ; 7
        rrca                            ; 4
        rrca                            ; 4
        rrca                            ; 4
        or      %00001000               ; 7
        out     ($FE),a                 ; 11: and voice 2 for the next 65
        djnz    .sample                 ; 13: 125 T-states a sample
        dec     c
        jr      nz,.sample
        ld      (Phase1),hl
        exx
        ld      (Phase2),hl
        exx
        ret

Interrupt:
        ret

Row:            dw      0
Phase1:         dw      0
Phase2:         dw      0
Step1:          dw      0
Step2:          dw      0
Text:           db      "Two voices, one bit",0

; Each note's step: its frequency x 65536 / the samples a second. That's not 3,500,000 / 125, as the counters stand
; still between one frame's 530 samples and the next's: it's 530 x 50.08 frames a second, 26,542 (on a 48K).
; The tune, a voice and a bass, one row each eight frames; 0 is silence.
Tune:
        dw      1628,323    ; E5  C3
        dw      1450,323    ; D5  C3
        dw      1292,242    ; C5  G2
        dw      1450,242    ; D5  G2
        dw      1628,323    ; E5  C3
        dw      1628,323    ; E5  C3
        dw      1628,242    ; E5  G2
        dw      0,242       ; -   G2
        dw      1450,242    ; D5  G2
        dw      1450,242    ; D5  G2
        dw      1450,363    ; D5  D3
        dw      0,363       ; -   D3
        dw      1628,323    ; E5  C3
        dw      1936,323    ; G5  C3
        dw      1936,407    ; G5  E3
        dw      0,407       ; -   E3
        dw      1628,323    ; E5  C3
        dw      1450,323    ; D5  C3
        dw      1292,242    ; C5  G2
        dw      1450,242    ; D5  G2
        dw      1628,323    ; E5  C3
        dw      1628,323    ; E5  C3
        dw      1628,242    ; E5  G2
        dw      1628,242    ; E5  G2
        dw      1450,242    ; D5  G2
        dw      1450,242    ; D5  G2
        dw      1628,242    ; E5  G2
        dw      1450,242    ; D5  G2
        dw      1292,323    ; C5  C3
        dw      0,323       ; -   C3
        dw      0,323       ; -   C3
        dw      0,0         ; -   -
TuneEnd:
