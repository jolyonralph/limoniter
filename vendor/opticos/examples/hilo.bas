' Guess my number (from the specification, section 4.7)
DEF FN fact(n)
  IF n <= 1 THEN RETURN 1
  RETURN n * fact(n - 1)
ENDFN
RANDOMIZE
secret% = RND(100)
tries% = 0
PRINT "I'm thinking of a number 1-100.  (10! = "; fact(10); ")"
REPEAT
  INPUT "Your guess? "; g$
  TRY
    g% = VAL(g$)
    IF g% < 1 OR g% > 100 THEN ERROR 100, "1 to 100 only, please"
  CATCH
    PRINT g$; ": "; ERR$
    g% = 0
  ENDTRY
  s% = SGN(g% - secret%)
  IF g% = 0 THEN s% = 9
  IF g% <> 0 THEN tries% = tries% + 1
  CASE s% OF
    WHEN -1: PRINT g%; " is too low"
    WHEN 1: PRINT g%; " is too high"
    WHEN 0: PRINT "Got it in "; tries%; " tries"
  ENDCASE
UNTIL s% = 0
