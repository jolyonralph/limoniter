' Read words, sort them, print them
DEF PROC words
  DATA "pear", "apple", "fig", "banana", "cherry"
ENDPROC
DIM w$(4)
RESTORE words
FOR i% = 0 TO 4: READ w$(i%): NEXT
REPEAT
  done% = TRUE
  FOR i% = 0 TO 3
    IF w$(i%) > w$(i% + 1) THEN SWAP w$(i%), w$(i% + 1): done% = FALSE
  NEXT
UNTIL done%
FOR i% = 0 TO 4: PRINT UPPER$(w$(i%)); " "; LEN w$(i%): NEXT
