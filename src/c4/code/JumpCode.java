package code;

/*
 * Jumpcode ist eine abstrakte Unterklasse von DreiAdrCode und fasst alle Sprungbefehle zusammen.
 * Jeder Befehl hat ein Sprungziel label.
 */

public abstract class JumpCode extends DreiAdrCode {
	int label;
	
	public int getLabel()
	{
		return label;
	}
	
	public void setLabel(int i)
	{
		label = i;
	}

}
