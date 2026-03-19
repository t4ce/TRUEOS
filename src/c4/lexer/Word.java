package lexer;

/*
 * Die Klasse Word beschreibt Eigenschaften von Identifier, reservierten Wörtern
 * und weiteren speziellen Tokenklassen.
 *
 * Zusätzlich zur Tokenklasse (codiert durch tag)
 * hat so ein Token auch einen Tokenwert, der  das zugehörige Lexeme ist. 
 * @author rp
 *
 */

public class Word extends Token {
	public String lexeme = "";

	public Word(String s, Tag tag) {
		super(tag);
		lexeme = s;
	}

	public Word(Tag tag) {
		super(tag);
		lexeme = tag.lexeme();
	}

	public static final Word 
		True = new Word(Tag.TRUE),
		False = new Word(Tag.FALSE),
		Uminus = new Word(Tag.UMINUS),
		toInt = new Word(Tag.TOI),
		toFloat = new Word(Tag.TOF),
		temp = new Word(Tag.TEMP);

	// für die Umwandlung Boolescher Ausdrücke bei expliziter CodeErzeugung
	public static final Word
		eq = new Word(Tag.EQ),
		ne = new Word(Tag.NEQ),
		ls = new Word(Tag.LESS),
		gr = new Word(Tag.GREATER),
		le = new Word("<=", Tag.LE),
		ge = new Word(">=", Tag.GE);

	public String toString() {
		return lexeme;
	}

}
