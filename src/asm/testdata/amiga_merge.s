; Code before any SECTION, sections used twice, an empty one, odd sizes, and chip and fast code.
		moveq	#1,d0
		lea	table,a0
		section	empty,data
		section	main,code
first:		moveq	#2,d1
		section	d,data_f
table:		dc.b	1
		section	main,code
		lea	table+1,a0
		dc.b	1
		section	fast,code_f
		jmp	first
		section	z,bss
		ds.b	3
		section	more,data
x:		dc.l	x,first,table
