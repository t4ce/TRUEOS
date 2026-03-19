package code;

import inter.Singleton;
import inter.Id;
import inter.Unary;

/*
 * Diese Klasse beschreibt Drei-Adress-Befehle der Form leftSide = operator rightSide
 */

public class Arith1OpCode extends ArithCode {
	Singleton rightSide;
		
	public Arith1OpCode(Id l, Unary u) {
		leftSide = l;
		operator = u.getOp();
		rightSide = (Singleton) u.getExpr();		
	}
	
	public String toString()
	{
		return (leftSide.toString() + " = " + operator.toString() + " " + rightSide.toString());
	}

}
