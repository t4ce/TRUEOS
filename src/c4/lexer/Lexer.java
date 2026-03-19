package lexer;

import java.util.*;


/*
 * Diese Klasse implementiert einen lexikalen Scanner für die
 * Beispielsprache. Die Instanzenvariablen sind
 * 		line für die momentane Zeilennummer der Eingabe
 * 		peek für das lookahead-Zeichen
 * 		prog für das Eingabeprogramm, ind zeigt auf das nächste Zeichen
 * 		words für eine Tabelle der reservierten Wörter und aller 
 * 			  		im Programm auftretenden Identifier. 
 * 		charTokenTable eine Tabelle aller Token, die aus einem oder zwei
 * 					Zeichen bestehen.
 */

public class Lexer {
	public static int line = 1;
	char peek = ' ';
	int ind = 0;
	char[] prog;

	/*
	 * charTokenTable ist eine Tabelle aller Token, die nur aus einem oder zwei Zeichen bestehen.
	 */
	Hashtable<String, Token> charTokenTable = new Hashtable<String, Token> ();

	/*
	 * words ist eine Tabelle aller reservierten Wörter und aller im Program auftretenden Identifier.
	 * Durch diese Tabelle ist gewährleistet, dass Identifier-Tokens eindeutig sind.
	 */
	Hashtable<String, Word> words = new Hashtable<String, Word>();	
	
	void reserve(Word w) {
		words.put(w.lexeme, w);
	}

	public Lexer(char[] p) {
		
		prog = p;
		
		// Auffüllen der der Tabelle mit den reservierten Wörtern		
		for (Tag t : Token.resWordTags) {
			reserve(new Word(t));			
		}
		// Eintragen der restlichen reservierten Wörter
		reserve(Word.False);
		reserve(Word.True);
		reserve(Type.Int);
		reserve(Type.Char);
		reserve(Type.Bool);
		reserve(Type.Float);

	    // Erzeugen der Tabelle aller Token, die nur aus einem oder zwei Symbolen bestehen
		for (Tag t : Token.charTags) {
			charTokenTable.put(t.lexeme(), new Token(t));
		}	
		
		
	}
	
	
	/*
	 * readch() liest das nächste Zeichen der Eingabe und speichert es in peek
	 */
	void readch(){
		peek = prog[ind++];
	}

	/*
	 * readch(char c) liest das nächste Zeichen der Eingabe und prüft, 
	 * ob es mit c übereinstimmt.
	 */

	boolean readch(char c)  {
		readch();
		if (peek != c)
			return false;
		peek = ' ';
		return true;
	}

	public Token scan() {
		for (;; readch()) {
			if (peek == ' ' || peek == '\t' || peek == '\r')
				continue;
			if (peek == '/') {
				readch();
				if (peek != '/') {
					return charTokenTable.get("/");
				}
				while (peek != '\n' && peek != 0) 
					readch();
			}
			if (peek == '\n')
				line = line + 1;
			else
				break;
		}


		/* hier werden Zahlen erkannt */

		if (Character.isDigit(peek)) {
			int v = 0;
			do {
				v = 10 * v + Character.digit(peek, 10);
				readch();
			} while (Character.isDigit(peek));
			if (peek != '.')
				return new Num(v);
			float x = v;
			v = 0;
			float div = 1;
			for (;;) {
				readch();
				if (!Character.isDigit(peek))
					break;
				v = 10*v + Character.digit(peek, 10);
				div = div * 10;
			}
			return new Real(x + v/div);		// wegen der Genauigkeit nur eine Division!
		}

		/* hier werden Symbole erkannt */

		if (Character.isLetter(peek)) {
			StringBuffer b = new StringBuffer();
			do {
				b.append(peek);
				readch();
			} while (Character.isLetterOrDigit(peek));
			String s = b.toString();
			Word w = (Word) words.get(s);  // Suche Wort in Tabelle
			if (w != null)
				return w;				   // Wort gefunden
			w = new Word(s, Tag.ID);
			words.put(s, w);				// Wort eintragen
			return w;
		}

		/* hier werden die Token erkannt, die nur aus einem oder zwei Zeichen bestehen */
		
		StringBuffer b = new StringBuffer();
		Token stok;
		b.append(peek);
		stok = charTokenTable.get(b.toString());
		if (stok != null) {
			Token oneCharToken = stok;
			readch();
			b.append(peek);
			stok = charTokenTable.get(b.toString());
			if (stok != null) {
				peek = ' ';
				return stok;
			} else {
				return oneCharToken;
			}
			
		}
		
		// das Ende der Eingabe wird durch einen 0-Character angezeigt

		if (peek == 0 ) {
			return new Token(Tag.EOF);
		}
        
		/* jedes andere Zeichen ist dem Scanner unbekannt und liefert NOTOKEN */

		Token tok = new Token(Tag.NOTOKEN);
		peek = ' ';
		return tok;

	}
}
