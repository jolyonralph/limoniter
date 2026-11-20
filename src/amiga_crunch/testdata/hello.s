; A small program in four hunks (code, data, chip data and BSS) with relocations between them, for testing
; decrunching: it prints a line made from all four, so it only prints right if every hunk and relocation is.
        section code,code
start:  move.l  4.w,a6
        move.l  #dosname,a1
        moveq   #0,d0
        jsr     -552(a6)                ; OpenLibrary
        move.l  d0,dosbase
        beq.s   .out
        move.l  d0,a6
        jsr     -60(a6)                 ; Output
        move.l  d0,d4
        ; Copy the message into BSS through a table of pointers, then the chip hunk's tail after it.
        lea     buffer,a2
        move.l  #table,a3
.next:  move.l  (a3)+,d0
        beq.s   .done
        move.l  d0,a0
.copy:  move.b  (a0)+,(a2)+
        bne.s   .copy
        subq.l  #1,a2
        bra.s   .next
.done:  move.l  a2,d3
        sub.l   #buffer,d3
        move.l  d4,d1
        move.l  #buffer,d2
        jsr     -48(a6)                 ; Write
        move.l  a6,a1
        move.l  4.w,a6
        jsr     -414(a6)                ; CloseLibrary
.out:   moveq   #0,d0
        rts
        rept    40
        nop
        endr

        section data,data
dosname: dc.b   "dos.library",0
first:  dc.b    "Decrunched program: the code, ",0
second: dc.b    "the data, ",0
        even
table:  dc.l    first,second,third,fourth,0
        dcb.b   600,$55
        dc.b    "padding that crunches well, padding that crunches well, padding that crunches well",0

        section chipdata,data_c
third:  dc.b    "the chip data ",0
fourth: dc.b    "and the BSS all work.",10,0
        even
        dcb.w   200,$1234

        section bss,bss
dosbase: ds.l   1
buffer: ds.b    256
