package code;

import inter.Singleton;
import inter.Logical;
import lexer.Token;


/*
 * Diese Klasse definiert bedingte Sprünge der Form if id1 op id2 goto label
 */


public class BedJumpCodeRel extends JumpCode {
	Singleton id1;
	Token op;
	Singleton id2;
	
	public BedJumpCodeRel(Logical exp, int i)
	{
		id1 = (Singleton)exp.getExpr1();
		op = exp.getOp();
		id2 = (Singleton)exp.getExpr2();
		label = i;
	}
	
	public String toString()
	{
		return ("if " + id1.toString() + op.toString() + id2.toString() + " goto " + label);
	}
}
