package lexer;

/*
 * Die Klasse beschreibt Eigenschaften von Integer-Zahlen.
 * Zusätzlich zur Tokenklasse (dargestellt durch das tag NUM) hat 
 * so ein Token auch einen Tokenwert, der in der Instanzenvariablen
 * value abgelegt wird.
 * @author rp
 */

public class Num extends Token {
	public final int value;

	public Num(int v) {
		super(Tag.NUM);
		value = v;
	}
	
	public String toString() {
		return "" + value;
	}

}
