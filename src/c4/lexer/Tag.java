package lexer;

/*
 * Eine Enumeration aller Tokenklassen, wobei das zugehörige Lexeme als Wert mit der Tokenklasse 
 * verknüpft wird. Dies wird für eine Tabelle der Token und die Ausgabe des Syntaxbaums usw. benötigt.
 */

public enum Tag {
	/* Alle Token, die nur aus einem Zeichen bestehen */
	LBRACE ("{"), 
	RBRACE  ("}"), 
	LBRACKET ("["), 
	RBRACKET ("]"), 
	LPARA ("("), 
	RPARA (")"),
	EQS ("="),
	LESS ("<"),
	GREATER (">"),
	PLUS ("+"),
	MINUS ("-"),
	DIV ("/"),
	MUL ("*"),
	NOT ("!"),
	ANDS ("&"),
	BAR ("|"),
	SEMI (";"),
	COMMA (","),
	
	/* Alle Token, die aus zwei Zeichen bestehen */
	AND ("&&"),
	OR ("||"),
	EQ ("=="),
	GE (">="),
	LE ("<="),
	NEQ ("!="),
	
	/* Reservierte Wörter */
	BREAK ("break"),
	DO ("do"),
	ELSE ("else"),
	FALSE ("false"),
	FOR ("for"),
	IF ("if"),
	TRUE ("true"),
	WHILE ("while"),
	
	/* Restliche und interne Tokenklassen */
	BASIC ("basic"),
	EOF ("eof"),
	ID ("id"),
	INDEX ("index"),
	NOTOKEN ("notoken"),
	NUM ("num"),
	REAL ("real"),
	UMINUS ("uminus"),
	TOI ("toi"),
	TOF ("tof"),
	TEMP ("temp")
;

	private final String lexeme;
	
	Tag (String l) {
		this.lexeme = l;
	}
	
	public String lexeme() { return lexeme; }
}
