; ch08_banks.asm: the 128K's eight banks of RAM, each paged in at $C000 in turn by port $7FFD, and timed. A small
; loop is copied into the bank (what was there is kept and put back after) and run for a frame; the times round it
; show which banks the ULA shares, and holds up: 1, 3, 5 and 7 on a 128K or +2, 4, 5, 6 and 7 on a +2A or +3.
; 128K, +2, +2A and +3 only. Chapter 8, The 128K and later models. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum128
        include "startup.asm"

AT              equ     $E000           ; where the loop goes in each bank: clear of the screen in bank 5 ($6000
                                        ; when it's at $4000) and of this program in bank 2 ($A000 at $8000)

Init:
        ld      a,%00111000
        call    ClearScreen
        ld      bc,$0101
        ld      hl,Heading
        call    PrintAt
        ld      bc,$0301
        ld      hl,Columns
        call    PrintAt
        ld      b,8                     ; a line for each bank: "Bank n"
.bank:
        push    bc
        ld      a,8
        sub     b
        push    af
        add     a,'0'
        ld      (Name+5),a
        pop     af
        add     a,5
        ld      b,a
        ld      c,1
        ld      hl,Name
        call    PrintAt
        pop     bc
        djnz    .bank
        ret

Frame:
        ld      a,(Which)
        inc     a
        and     7
        ld      (Which),a
        ; The bank at $C000, the ROM and screen kept as BASIC had them.
        ld      b,a
        ld      a,(BANKM)
        and     %11111000
        or      b
        ld      bc,$7FFD
        out     (c),a
        ; What's at AT kept, the loop copied there, run until the next interrupt, and what was there put back.
        ld      hl,AT
        ld      de,Kept
        ld      bc,CountEnd-Count
        ldir
        ld      hl,Count
        ld      de,AT
        ld      bc,CountEnd-Count
        ldir
        call    AT
        push    hl
        ld      hl,Kept
        ld      de,AT
        ld      bc,CountEnd-Count
        ldir
        pop     hl
        ld      a,(Which)
        add     a,5
        ld      b,a
        ld      c,26
        jp      PrintNumber

; The loop timed: 35 T-states a time round when nothing holds it up, counting in HL until the interrupt adds one
; to Frames. Only relative jumps, so it runs wherever it's copied.
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

Interrupt:
        ret

Which:          db      7
Heading:        db      "Each bank at $C000, timed",0
Columns:        db      "                   Times round",0
Name:           db      "Bank 0",0
Kept:           ds      CountEnd-Count
