package lexer;

/*
 * Diese Klasse beschreibt weitere reservierte Worte, in diesem Fall die
 * Typbezeichner. Als zusätzliche Instanzenvariable ist die Größe eines
 * Objekts des jeweiligen Typs in Bytes angegeben. Auch hier werden die 
 * Tokenklassen als Klassenvariablen zur Verfügung gestellt. 
 * 
 */

public class Type extends Word {
	public int width = 0; // for storage allocation

	public Type(String s, Tag tag, int w) {
		super(s, tag);
		width = w;
	}

	public int getWidth() {
		return width;
	}

	public static final Type 
	    Int   = new Type("int", 	Tag.BASIC, 4),
	    Float = new Type("float", 	Tag.BASIC, 8), 
	    Char  = new Type("char",	Tag.BASIC, 1),
	    Bool  = new Type("bool", 	Tag.BASIC, 1);
}
