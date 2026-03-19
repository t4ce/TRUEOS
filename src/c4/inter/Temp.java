package inter;

import lexer.Word;
import lexer.Type;


public class Temp extends Id {
	static int count = 0;

	int number; // temporäre Variable haben die lexikale Darstellung
				// "t <number>"

	public Temp(Type p) {
		super(Word.temp, p, 0);
		number = ++count;
	}

	public String toString() {
		return "t" + number;
	}

	// falls mehrere CodeGeneratoren nacheinander verwendet werden sollen
	public static void resetCounter() {
		count = 0;
	}

}
