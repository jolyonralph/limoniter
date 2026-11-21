; ch03_contention.asm: contended memory, measured. The same small loop is run for one whole frame from $8000
; (never contended) and from $6000 (contended, shared with the ULA), and the times round each is shown. On a 128K
; the loop is also run from $C000 with bank 1 paged in (contended) and bank 0 (not).
; Chapter 3, The machine. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

CONTENDED       equ     $6000           ; in bank 5, which the ULA reads the screen from

Init:
        ld      a,%00111000
        call    ClearScreen
        ld      hl,Texts                ; each line: its row, its column, its text
.text:
        ld      b,(hl)
        inc     b
        jr      z,.copies               ; a row of $FF ends them
        dec     b
        inc     hl
        ld      c,(hl)
        inc     hl
        ld      a,b
        cp      7                       ; rows 7 and 8, the 128K's, only on a 128K
        jr      c,.print
        ld      a,(Is128)
        or      a
        jr      z,.copies
.print:
        call    PrintAt
        inc     hl                      ; past the 0 that ended it
        jr      .text
.copies:
        ; The loop, copied to the contended address (and on a 128K, to banks 1 and 0 at $C000).
        ld      de,CONTENDED
        call    CopyLoop
        ld      a,(Is128)
        or      a
        ret     z
        ld      a,1
        call    Page
        ld      de,$C000
        call    CopyLoop
        xor     a
        call    Page
        ld      de,$C000
        jp      CopyLoop

CopyLoop:
        ld      hl,Count
        ld      bc,CountEnd-Count
        ldir
        ret

; Page: bank A (0 to 7) at $C000, the ROM and screen left as BASIC had them.
Page:
        ld      b,a
        ld      a,(BANKM)
        and     %11111000
        or      b
        ld      bc,$7FFD
        out     (c),a
        ret

; Each frame, one of the places is timed: Frame is called just after the interrupt, and the loop runs until the
; next one. The count it reaches is printed on the frame after.
Frame:
        ld      a,(Which)
        inc     a
        ld      hl,Is128
        ld      b,2                     ; two places on a 48K, four on a 128K
        bit     0,(hl)
        jr      z,.places
        ld      b,4
.places:
        cp      b
        jr      c,.this
        xor     a
.this:
        ld      (Which),a
        ld      hl,Count                ; place 0: $8000's copy
        or      a
        jr      z,.run
        ld      hl,CONTENDED            ; place 1
        dec     a
        jr      z,.run
        dec     a                       ; places 2 and 3: bank 1, then bank 0, at $C000
        xor     1
        call    Page
        ld      hl,$C000
.run:
        call    .jump
        ld      a,(Which)               ; the count, printed beside its place
        add     a,5                     ; rows 5 to 8
        ld      b,a
        ld      c,26
        jp      PrintNumber
.jump:
        jp      (hl)

; The loop timed: 35 T-states a time round when nothing holds it up. It counts in HL until the interrupt adds one
; to Frames. It uses only relative jumps, so it runs wherever it's copied.
Count:
        ld      a,(Frames)
        ld      b,a
        ld      hl,0
.loop:
        inc     hl                      ; 6
        ld      a,(Frames)              ; 13
        cp      b                       ; 4
        jr      z,.loop                 ; 12
        ret
CountEnd:

Interrupt:                              ; nothing to do in the interrupt
        ret

Which:          db      0
Texts:          db      1,1,"Times round a loop in a frame",0
                db      3,1,"Where         Contended? Times",0
                db      5,1,"$8000         no",0
                db      6,1,"$6000         yes",0
                db      7,1,"$C000 bank 1  yes",0
                db      8,1,"$C000 bank 0  no",0
                db      $FF
