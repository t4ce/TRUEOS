package code;

import inter.Singleton;
import inter.Id;

/*
 * Diese Klasse definiert Drei-Adress Befejle der Form leftSide = rightSide
 * rightSide ist dabei eine Konstante oder ein Identifier
 */

public class AssignCode extends ArithCode {
	Singleton rightSide;
	
	public AssignCode(Id l, Singleton r) {
		leftSide = l;
		rightSide = r;
	}
	
	public String toString()
	{
		return (leftSide.toString() + " = " + rightSide.toString());
	}

}
