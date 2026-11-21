; ch11_next_copper.asm: the Next's copper, a coprocessor that waits for a place on the screen and then writes a
; nextreg, in step with the beam, while the Z80 does something else. Its list here changes palette entry 16 (the
; colour of PAPER 0, which the border uses too when it's black) on every one of the 312 lines, for a rainbow
; down the border and behind the text: three instructions a line (WAIT for the line, MOVE the index to nextreg
; $40, MOVE the colour to $41), 936 of the copper's 1,024. Each frame the list is made again, a line further on,
; and sent to the copper through nextreg $60 while it runs. For the Next only. Chapter 11, The Next. CC0.

AUTOEXIT        equ     0               ; not used on the Next: this runs until the Next is reset

        device  zxspectrumnext
        org     $8000

NEXTREG_SELECT  equ     $243B
LINES           equ     312

Start:
        di
        nextreg $07,3                   ; 28 MHz
        xor     a
        out     ($FE),a                 ; a black border, so it's PAPER 0's colour
        ld      hl,$4000                ; the screen clear, white ink on black paper
        ld      de,$4001
        ld      bc,6144
        ld      (hl),0
        ldir
        ld      (hl),%00000111
        ld      bc,767
        ldir
        ld      hl,Text
        ld      de,$4000+10*32+8        ; character row 10, column 8 (rows 8 to 15 are 2048 bytes on)
        call    Print
        nextreg $43,%00000000           ; palette control: the ULA's first palette, auto-increment on
        ; The first list, sent with the copper stopped, then started: from index 0, and again from index 0 each
        ; time the raster reaches the top of the frame (start control %11).
        nextreg $62,%00000000
        call    MakeList
        call    SendList
        nextreg $62,%11000000
Frame:
        call    WaitFrame
        ld      a,(Offset)              ; the colours a line further on
        inc     a
        ld      (Offset),a
        call    MakeList
        call    SendList                ; sent while the copper runs: it isn't stopped, so it isn't restarted
        jr      Frame

; SendList: the list to the copper's memory from address 0. Writing $62 with the start control it already has
; changes only the address, and doesn't restart the copper. Each byte goes to nextreg $60, which moves the address
; on by itself, with the Next's OUTINB: OUT (C),(HL) and INC HL, leaving B (the port's high byte) alone.
SendList:
        nextreg $61,0
        ld      bc,NEXTREG_SELECT
        ld      a,$62
        out     (c),a
        inc     b
        in      a,(c)                   ; the start control as it is, address bits cleared
        and     %11000000
        out     (c),a
        dec     b
        ld      a,$60
        out     (c),a
        inc     b                       ; $253B, the data port
        ld      hl,List
        ld      e,(ListEnd-List)/8      ; 8 bytes a time round: 234 times
.send:
        outinb
        outinb
        outinb
        outinb
        outinb
        outinb
        outinb
        outinb
        dec     e
        jr      nz,.send
        ret

; MakeList: for each line, WAIT for its start, then MOVE 16 to $40 and the line's colour to $41.
;   WAIT: %1hhhhhhv %vvvvvvvv, the horizontal place h x 8 and the line v (0 to 311; line 0 is the screen's top)
;   MOVE: %0rrrrrrr %vvvvvvvv, the nextreg r and the value
MakeList:
        ld      ix,List
        ld      hl,0                    ; the line
        ld      a,(Offset)
        ld      e,a
.line:
        ld      a,h                     ; WAIT: bit 0 of the first byte is v's top bit
        and     1
        or      %10000000               ; h = 0: the start of the line
        ld      (ix+0),a
        ld      (ix+1),l
        ld      (ix+2),$40              ; MOVE $40,16
        ld      (ix+3),16
        ld      (ix+4),$41              ; MOVE $41,colour
        ld      a,l
        add     a,e
        push    hl
        ld      h,Colours/256
        ld      l,a
        ld      a,(hl)
        pop     hl
        ld      (ix+5),a
        ld      bc,6
        add     ix,bc
        inc     hl
        ld      bc,LINES
        or      a
        push    hl
        sbc     hl,bc
        pop     hl
        jr      nz,.line
        ret

; Print: the text at HL (ending with 0) at screen address DE, in the ROM's font.
Print:
        ld      a,(hl)
        or      a
        ret     z
        push    hl
        push    de
        ld      l,a
        ld      h,0
        add     hl,hl
        add     hl,hl
        add     hl,hl
        ld      bc,$3C00
        add     hl,bc
        ld      b,8
.line:
        ld      a,(hl)
        ld      (de),a
        inc     hl
        inc     d
        djnz    .line
        pop     de
        pop     hl
        inc     hl
        inc     e
        jr      Print

; WaitFrame: until the video reaches line 192, then until it's left it (nextreg $1F, the line's low byte).
WaitFrame:
        ld      bc,NEXTREG_SELECT
        ld      a,$1F
        out     (c),a
        inc     b
.wait:
        in      a,(c)
        cp      192
        jr      nz,.wait
.leave:
        in      a,(c)
        cp      192
        jr      z,.leave
        ret

Offset:         db      0
Text:           db      "The Next's copper",0

; 256 colours, RRRGGGBB, twice round the colour wheel: one for each line, moving a line each frame.
        align   256
Colours:
        db      $E0,$E0,$E4,$E4,$E4,$E8,$E8,$E8,$EC,$EC,$EC,$F0,$F0,$F0,$F4,$F4
        db      $F4,$F8,$F8,$F8,$FC,$FC,$FC,$DC,$DC,$DC,$BC,$BC,$BC,$9C,$9C,$9C
        db      $9C,$7C,$7C,$7C,$5C,$5C,$5C,$3C,$3C,$3C,$1C,$1C,$1C,$1C,$1C,$1D
        db      $1D,$1D,$1D,$1D,$1D,$1D,$1E,$1E,$1E,$1E,$1E,$1E,$1E,$1F,$1F,$1F
        db      $1F,$1F,$1B,$1B,$1B,$17,$17,$17,$13,$13,$13,$0F,$0F,$0F,$0B,$0B
        db      $0B,$07,$07,$07,$03,$03,$03,$23,$23,$23,$43,$43,$43,$63,$63,$63
        db      $83,$83,$83,$83,$A3,$A3,$A3,$C3,$C3,$C3,$E3,$E3,$E3,$E3,$E3,$E2
        db      $E2,$E2,$E2,$E2,$E2,$E2,$E1,$E1,$E1,$E1,$E1,$E1,$E1,$E0,$E0,$E0
        db      $E0,$E0,$E4,$E4,$E4,$E8,$E8,$E8,$EC,$EC,$EC,$F0,$F0,$F0,$F4,$F4
        db      $F4,$F8,$F8,$F8,$FC,$FC,$FC,$DC,$DC,$DC,$BC,$BC,$BC,$9C,$9C,$9C
        db      $9C,$7C,$7C,$7C,$5C,$5C,$5C,$3C,$3C,$3C,$1C,$1C,$1C,$1C,$1C,$1D
        db      $1D,$1D,$1D,$1D,$1D,$1D,$1E,$1E,$1E,$1E,$1E,$1E,$1E,$1F,$1F,$1F
        db      $1F,$1F,$1B,$1B,$1B,$17,$17,$17,$13,$13,$13,$0F,$0F,$0F,$0B,$0B
        db      $0B,$07,$07,$07,$03,$03,$03,$23,$23,$23,$43,$43,$43,$63,$63,$63
        db      $83,$83,$83,$83,$A3,$A3,$A3,$C3,$C3,$C3,$E3,$E3,$E3,$E3,$E3,$E2
        db      $E2,$E2,$E2,$E2,$E2,$E2,$E1,$E1,$E1,$E1,$E1,$E1,$E1,$E0,$E0,$E0
List:           ds      LINES*6         ; the copper list, made by MakeList
ListEnd:
