package code;

import inter.Singleton;
import inter.Id;
import inter.Arith;


/*
 * Diese Klasse beschreibt  Drei-Adress-Befehle der Form leftSide = e1 operator e2
 */

public class Arith2OpCode extends ArithCode {
	Singleton e1;
	Singleton e2;
	
	public Arith2OpCode(Id l, Arith exp){
		leftSide = l;
		e1 = (Singleton)exp.getExpr1();
		e2 = (Singleton)exp.getExpr2();
		operator = exp.getOp();		
	}
	
	public String toString()
	{
		return (leftSide.toString() + " = " + e1.toString() + " " + operator.toString() + " " + e2.toString());
	}

}
