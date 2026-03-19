package inter;

import lexer.Token;
import lexer.Type;
import code.DreiAdrCode;
import code.AssignCode;

	/*
	 * Singleton ist eine abstrakte Unterklasse von Expr und beschreibt
	 * elementare Ausdrücke wie Identifier oder Konstanten. 
	 */

public abstract class Singleton extends Expr {

	Singleton(Token t, Type p) {
		super(t, p);
	}
	
	// für die Drei-Adress-Code Erzeugung
	
	public DreiAdrCode codeForValueTo(Id id) {
		return (new AssignCode(id, this));
	}

	public boolean isSingleton()
	{
		return true;
	}

		
	}
	
