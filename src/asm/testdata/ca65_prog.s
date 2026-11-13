; A ca65 program
        .setcpu "6502"
        .org $c000
SCREEN = $0400
COLOUR := $d020

.proc main
        ldx #0
@loop:  lda message,x
        beq @done
        sta SCREEN,x
        inx
        bne @loop
@done:  inc COLOUR
        jmp @done
.endproc

message: .byte "HELLO", 0
table:   .word main, message
         .res 3, $ea
:       dex
        bne :-
        rts
