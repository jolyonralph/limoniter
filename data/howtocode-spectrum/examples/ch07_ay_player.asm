; ch07_ay_player.asm: a small music player for the 128K's AY-3-8912, written here. Three channels: a tune on
; A, a bass on B, and a drum on C made from the AY's noise. A row of the tune every six frames; between rows each
; note fades, a step a frame, as the player turns its volume down. All fourteen registers it uses are kept in
; memory and written to the AY together, once a frame, from the interrupt. 128K, +2, +2A and +3 only.
; Chapter 7, Sound. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum128
        include "startup.asm"

ROW_FRAMES      equ     6
AY_SELECT       equ     $FFFD           ; OUT: which register; IN: read it
AY_WRITE        equ     $BFFD           ; OUT: write the register chosen
MIXER_TONES     equ     %00111000       ; R7: tones on A, B and C (0 is on), noise off
MIXER_DRUM      equ     %00011100       ; tones on A and B, noise on C and its tone off

Init:
        ld      a,%00111000
        call    ClearScreen
        ld      bc,$0A03
        ld      hl,Text
        call    PrintAt
        ld      hl,Tune
        ld      (Row),hl
        ld      a,1
        ld      (Tick),a
        ld      a,MIXER_TONES
        ld      (Regs+7),a
        ld      a,6                     ; R6: the noise's period, 0 to 31: a short hiss
        ld      (Regs+6),a
        ret

Frame:
        ret

Interrupt:
        ld      hl,Tick
        dec     (hl)
        jr      nz,Fade
        ld      (hl),ROW_FRAMES
        ; A new row: three bytes, the note for A, for B (0 for none), and 255 for a drum on C.
        ld      hl,(Row)
        ld      a,(hl)
        ld      ix,Regs+0               ; A's period is R0 and R1, its volume R8
        ld      iy,Regs+8
        call    Note
        inc     hl
        ld      a,(hl)
        ld      ix,Regs+2               ; B's: R2 and R3, and R9
        ld      iy,Regs+9
        call    Note
        inc     hl
        ld      a,(hl)
        inc     hl
        or      a
        jr      z,.no_drum
        ld      a,15
        ld      (Regs+10),a
        ld      a,MIXER_DRUM
        ld      (Regs+7),a
.no_drum:
        ld      de,TuneEnd
        or      a
        push    hl
        sbc     hl,de
        pop     hl
        jr      nz,.next
        ld      hl,Tune
.next:
        ld      (Row),hl
        jr      Write

; Each frame between rows, every channel a step quieter; when the drum has faded, C's noise is turned off.
Fade:
        ld      hl,Regs+8
        ld      b,3
.channel:
        ld      a,(hl)
        or      a
        jr      z,.quiet
        dec     (hl)
.quiet:
        inc     hl
        djnz    .channel
        ld      a,(Regs+10)
        cp      10
        jr      nc,Write
        ld      a,MIXER_TONES
        ld      (Regs+7),a
        xor     a
        ld      (Regs+10),a

; All the registers, R0 to R10, written: register number to $FFFD, then its value to $BFFD.
Write:
        ld      hl,Regs
        ld      e,0
.register:
        ld      bc,AY_SELECT
        out     (c),e
        ld      a,(hl)
        ld      b,AY_WRITE/256          ; C is $FD for both ports
        out     (c),a
        inc     hl
        inc     e
        ld      a,e
        cp      11
        jr      nz,.register
        ret

; Note: A, a note's number (0 for none), started on a channel: its period from the table into IX and IX+1, and
; its volume, at IY, full.
Note:
        or      a
        ret     z
        push    hl
        dec     a
        add     a,a
        ld      e,a
        ld      d,0
        ld      hl,Periods
        add     hl,de
        ld      a,(hl)
        ld      (ix+0),a
        inc     hl
        ld      a,(hl)
        ld      (ix+1),a
        pop     hl
        ld      (iy+0),15               ; and its volume, full
        ret

Row:            dw      0
Tick:           db      0
Regs:           ds      11              ; R0 to R10: the periods of A, B and C, noise, mixer, and the volumes
Text:           db      "The AY: tune, bass, drum",0

; Each note's period: the AY's clock on a 128K (1,773,400 Hz) / (16 x its frequency).
Periods:
        dw      1345,1270,1131,1008,847,755,336,317,283,267,252,224,212,189,168
; E2 F2 G2 A2 C3 D3 E4 F4 G4 G#4 A4 B4 C5 D5 E5

; The tune: for each row, A's note, B's note (numbers in Periods from 1; 0 for none) and 255 for a drum.
Tune:
        db      11,4,255    ; A4  A2  *
        db      0,0,0       ; -   -   -
        db      13,0,0      ; C5  -   -
        db      0,0,0       ; -   -   -
        db      15,4,255    ; E5  A2  *
        db      0,0,0       ; -   -   -
        db      13,0,0      ; C5  -   -
        db      0,0,0       ; -   -   -
        db      9,3,255     ; G4  G2  *
        db      0,0,0       ; -   -   -
        db      12,0,0      ; B4  -   -
        db      0,0,0       ; -   -   -
        db      14,3,255    ; D5  G2  *
        db      0,0,0       ; -   -   -
        db      12,0,0      ; B4  -   -
        db      0,0,0       ; -   -   -
        db      8,2,255     ; F4  F2  *
        db      0,0,0       ; -   -   -
        db      11,0,0      ; A4  -   -
        db      0,0,0       ; -   -   -
        db      13,2,255    ; C5  F2  *
        db      0,0,0       ; -   -   -
        db      11,0,0      ; A4  -   -
        db      0,0,0       ; -   -   -
        db      7,1,255     ; E4  E2  *
        db      0,0,0       ; -   -   -
        db      10,0,0      ; G#4 -   -
        db      0,0,0       ; -   -   -
        db      12,1,255    ; B4  E2  *
        db      0,0,0       ; -   -   -
        db      15,1,255    ; E5  E2  *
        db      0,0,255     ; -   -   *
TuneEnd:
