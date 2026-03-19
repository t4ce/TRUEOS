package lexer;

/*
 * Die Klasse beschreibt Eigenschaften von Real-Zahlen.
 * Zusätzlich zur Tokenklasse (dargestellt durch das tag REAL) hat 
 * so ein Token auch einen Tokenwert, der in der Instanzenvariablen
 * value abgelegt wird.
 * @author rp
 */

public class Real extends Token {
	public final float value;

	public Real(float v) {
		super(Tag.REAL);
		value = v;
	}
	
	public String toString() {
		return "" + value;
	}

}
