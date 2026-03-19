package lexer;

/* 
 * Die Klasse Token beschreibt allgemeine Eigenschaften eines Tokens.
 * Jedes Token hat ein tag (ein Element der Enumeration Tag), das die Tokenklasse
 * codiert. 
 * @author rp
 */

public class Token {
	

	public final Tag tag;

	public Token(Tag t) {
		tag = t;
	}

	/* Die folgenden Arrays werden für die Initialisierung des Scanners benötigt */
	static Tag[] charTags = { Tag.LBRACE, Tag.RBRACE, Tag.LBRACKET, Tag.RBRACKET, Tag.LPARA, Tag.RPARA, Tag.EQS, Tag.BAR, 
			Tag.LESS, Tag.DIV, Tag.GREATER, Tag.PLUS, Tag.MINUS, Tag.MUL, Tag.NOT, Tag.ANDS, Tag.SEMI, Tag.COMMA,
			Tag.AND, Tag.OR, Tag.EQ, Tag.GE, Tag.LE, Tag.NEQ } ;

	static Tag[] resWordTags = { Tag.BREAK, Tag.DO, Tag.ELSE, Tag.FALSE, Tag.FOR, Tag.IF, Tag.TRUE, Tag.WHILE } ; // true und false werden in Word
																										// int, float, char und bool in Type definiert


	static Tag[] otherTags = { Tag.BASIC, Tag.EOF, Tag.ID, Tag.INDEX, Tag.NOTOKEN, Tag.NUM, Tag.REAL, Tag.UMINUS,
			Tag.TOI, Tag.TOF, Tag.TEMP }; 

	public String toString() {
		return tag.lexeme();
	}
}
