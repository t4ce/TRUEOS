package inter;

import lexer.*;
import code.DreiAdrCode;

/*
 * Expr ist eine abstrakte Unterklasse von Node und beschreibt Ausdrücke. 
 * In der Instanzenvariable op wird die jeweilige Operation 
 * oder der jeweilige Operand abgelegt
 */

public abstract class Expr extends Node {
	Token op;
	Type type;

	Expr(Token tok, Type p) {
		op = tok;
		type = p;
	}

	public Token getOp() {
		return op;
	}
	
	public Type getType() {
		return type;
	}

	public void setOp(Token op) {
		this.op = op;
	}
	public void setType(Type type) {
		this.type = type;
	}

	// für explizite Codeerzeugung:
	public boolean isSingleton()
	{
		return false;
	}

	public boolean isConstant()
	{
		return false;
	}

	public DreiAdrCode codeForValueTo(Id id) {
		System.out.println("codeForValueTo an falschen Knotentyp: " + this.getClass().toString());
		return null;
	}

	// Zur textuellen Ausgabe von Operation oder Operand
	public String toString() {
		return op.toString();
	}

	
}
