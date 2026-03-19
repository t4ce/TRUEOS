package code;

import inter.Singleton;
import inter.Id;
import inter.Access;

/*
 * Diese Klasse definiert Drei-Adress-Befehle der Form leftSide = array[index] 
 */

public class ArrayRefCode extends ArrayCode {
	Id leftSide;
	
	public ArrayRefCode(Id left, Access acc) {
		leftSide = left;
		array = (Id) acc.getArray();
		index = (Singleton)acc.getIndex();		
	}
	
	public String toString()
	{
		return (leftSide.toString() + " = " + array.toString() + "[" + index.toString() + "]");
	}
}
